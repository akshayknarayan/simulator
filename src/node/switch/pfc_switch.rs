use std::vec::Vec;

use slog;

use ::{Nanos, Result};
use event::Event;
use node::{NodeTransmitEvent, Link};
use packet::Packet;
use super::{Switch, PFCSwitchFamily, Queue};

/// PFCSwitch uses a *static* and *queue-agnostic* PFC threshold.
/// This means that once the queue headroom decreases below a static threshold, it PAUSEs *all*
/// incoming queues.
/// It resumes the incoming queues (all at once) when headroom rises above the static
/// `pfc_resume_threshold`.
#[derive(Default, Debug)]
pub struct PFCSwitch {
    pub id: u32,
    pub active: bool,
    pub rack: Vec<(Box<Queue>, bool)>, // a queue to send, and whether we have paused the corresponding incoming queue 
    pub core: Vec<(Box<Queue>, bool)>, // a queue to send, and whether we have paused the corresponding incoming queue 
}

impl PFCSwitchFamily for PFCSwitch {}

impl PFCSwitch {
    fn pause_incoming(&mut self, time: Nanos, logger: Option<&slog::Logger>) {
        let id = self.id;

        self.rack
            .iter_mut()
            .chain(self.core.iter_mut())
            .filter(|(_, already_paused)| !already_paused)
            .for_each(|(q, ref mut already_paused)| {
                //   --->
                // A      B ---> C
                //   <---
                //
                // TODO
                // following sequence of events:
                // 1. A starts tx of packet P to B
                // 2. B -> C queue fills
                // 3. B sends PAUSE to A, but it is corrupted (possible?)
                // 4. B receives packet P
                // 5. B sends PAUSE again
                if let Some(log) = logger {
                    debug!(log, "tx";
                        "time" => time,
                        "node" => id,
                        "packet" => ?Packet::Pause(id),
                    );
                }

                // send pause to upstream queue
                *already_paused = true;
                q.force_tx_next(Packet::Pause(id)).unwrap();
            });
    }

    fn resume_incoming(&mut self, time: Nanos, logger: Option<&slog::Logger>) {
        let id = self.id;

        self.rack
            .iter_mut()
            .chain(self.core.iter_mut())
            .filter(|(_, already_paused)| *already_paused)
            .for_each(|(q, ref mut already_paused)| {
                if let Some(log) = logger {
                    debug!(log, "tx";
                        "time" => time,
                        "node" => id,
                        "packet" => ?Packet::Resume(id), 
                    );
                }

                *already_paused = false;
                q.force_tx_next(Packet::Resume(id)).unwrap();
            });
    }
}

impl Switch for PFCSwitch {
    fn new(
        switch_id: u32,
        links: impl Iterator<Item=Box<Queue>>,
    ) -> Self {
        PFCSwitch{
            id: switch_id,
            active: false,
            rack: links.map(|q| (q, false)).collect::<Vec<(Box<Queue>, bool)>>(),
            core: vec![],
        }
    }

    fn id(&self) -> u32 {
        self.id
    }

    fn receive(
        &mut self, 
        p: Packet, 
        _l: Link, 
        time: Nanos, 
        logger: Option<&slog::Logger>,
    ) -> Result<Vec<Box<Event>>> {
        self.active = true;
        let id = self.id;
        if let Some(log) = logger {
            debug!(log, "rx";
                "time" => time,
                "node" => self.id,
                "packet" => ?p,
            );
        }

        // switches are output queued
        match p {
            Packet::Pause(from) => {
				self.rack
					.iter_mut()
					.find(|(ref q, _)| {
						let link_src = q.link().from;
						link_src == from
					})
					.map_or_else(|| unimplemented!(), |(rack_link_queue, _)| {
                        rack_link_queue.set_paused(true);
                    });

                Ok(vec![])
			}
			Packet::Resume(from) => {
				self.rack
					.iter_mut()
					.find(|(ref q, _)| {
						let link_src = q.link().from;
						link_src == from
					})
					.map_or_else(|| unimplemented!(), |(rack_link_queue, _)| {
                        rack_link_queue.set_paused(false);
                    });

                Ok(vec![])
			},
            Packet::Nack{hdr, ..} |
            Packet::Ack{hdr, ..} |
            Packet::Data{hdr, ..} => {
                let mut should_pause = false;
				self.rack
                    .iter_mut()
                    .find(|(ref q, _)| {
                        let link_dst = q.link().to;
                        link_dst == hdr.to
                    })
					.map_or_else(|| unimplemented!(), |(rack_link_queue, _)| {
						// send packet out on rack_link_queue
						if let None = rack_link_queue.enqueue(p) {
                            // packet was dropped
                            if let Some(log) = logger {
                                debug!(log, "dropping";
                                    "time" => time,
                                    "node" => id,
                                    "packet" => ?p,
                                );
                            }

                            return;
                        }

                        if rack_link_queue.headroom() <= rack_link_queue.link().pfc_pause_threshold() {
                            // outgoing queue has filled up
                            should_pause = true;
                        }
					});
                
                if should_pause {
                    self.pause_incoming(time, logger);
                }

                Ok(vec![])
            }
        }
    }

    fn exec(&mut self, time: Nanos, logger: Option<&slog::Logger>) -> Result<Vec<Box<Event>>> {
        // step all queues forward
        let mut should_resume = false;
        let id = self.id;
        let evs = self.rack.iter_mut().chain(self.core.iter_mut())
            .filter(|(q, _)| {
                q.is_active()
            })
            .filter_map(|(q, _)| {
                q.set_active(false);
                if let Some(pkt) = q.dequeue() {
                    // check if queue is sufficiently empty
                    if q.headroom() > q.link().pfc_resume_threshold() {
                        should_resume = true;
                    }

                    if let Some(log) = logger {
                        debug!(log, "tx";
                            "time" => time,
                            "node" => id,
                            "packet" => ?pkt,
                        );
                    }

                    Some(
                        Box::new(
                            NodeTransmitEvent(q.link(), pkt)
                        ) as Box<Event>,
                    )
                } else {
                    None
                }
            })
            .collect::<Vec<Box<Event>>>();

        if should_resume {
            self.resume_incoming(time, logger);
        }

        Ok(evs)
    }
    
    fn reactivate(&mut self, l: Link) {
        assert_eq!(l.from, self.id);
        self.rack.iter_mut()
            .chain(self.core.iter_mut())
            .find(|(ref q, _)| {
                q.link().to == l.to
            })
            .map_or_else(|| unimplemented!(), |(link_queue, _)| {
                link_queue.set_active(true);
            });
    }

    fn is_active(&self) -> bool {
        self.active
    }
}

use std::collections::HashMap;

#[derive(Default, Debug)]
pub struct IngressPFCSwitch(PFCSwitch, HashMap<u32, u32>, HashMap<Packet, u32>);

impl PFCSwitchFamily for IngressPFCSwitch {}

impl Switch for IngressPFCSwitch {
    fn new(
        switch_id: u32,
        links: impl Iterator<Item=Box<Queue>>,
    ) -> Self {
        IngressPFCSwitch(PFCSwitch::new(switch_id, links), HashMap::new(), HashMap::new())
    }
    
    fn id(&self) -> u32 {
        self.0.id()
    }
    
    fn receive(
        &mut self, 
        p: Packet, 
        l: Link, 
        time: Nanos, 
        logger: Option<&slog::Logger>,
    ) -> Result<Vec<Box<Event>>> {
        match p {
            Packet::Pause(_) |
			Packet::Resume(_) => {
                self.0.receive(p, l, time, logger)
			},
            Packet::Nack{hdr, ..} |
            Packet::Ack{hdr, ..} |
            Packet::Data{hdr, ..} => {
                let id = self.id();
                self.0.active = true;
                let ingress_queues = &mut self.1;
                let ingress_queue_mapping = &mut self.2;
                let num_links = self.0.rack.len();
                let mut queue_to_pause: Option<u32> = None;

				self.0.rack
                    .iter_mut()
                    .find(|(ref q, _)| {
                        let link_dst = q.link().to;
                        link_dst == hdr.to
                    })
					.map_or_else(|| unimplemented!(), |(out_queue, _)| {
                        // already_paused corresponds to the other-direction incoming queue on this
                        // link
                        //
						// send packet out on out_queue
						if let None = out_queue.enqueue(p) {
                            // packet was dropped
                            if let Some(log) = logger {
                                debug!(log, "dropping";
                                    "time" => time,
                                    "node" => id,
                                    "packet" => ?p,
                                );
                            }

                            return;
                        } else {
                            ingress_queue_mapping.entry(p).or_insert(l.from);
                            let virtual_ingress_queue_occupancy = ingress_queues
                                .entry(l.from)
                                .and_modify(|occ| { *occ += p.get_size_bytes(); })
                                .or_insert(p.get_size_bytes());

                            // TODO check that this is correct
                            //let per_ingress_static_pfc_thresh = (out_queue.headroom() as f64 / num_links as f64) as u32 - out_queue.link().pfc_pause_threshold();
                            //let per_ingress_static_pfc_thresh = (out_queue.link().pfc_pause_threshold() as f64 * num_links as f64) as u32;
                            let per_ingress_static_pfc_thresh = (out_queue.headroom() as f64 / num_links as f64) as u32;
                            if *virtual_ingress_queue_occupancy + out_queue.link().pfc_pause_threshold() > per_ingress_static_pfc_thresh {
                                // PAUSE this ingress queue
                                queue_to_pause = Some(l.from);
                            }

                            //if let Some(log) = logger {
                            //    debug!(log, "enqueue";
                            //        "headroom" => out_queue.headroom(),
                            //        "ingress-occupancy" => *virtual_ingress_queue_occupancy,
                            //        "in_link" => ?l,
                            //        "out_link" => ?out_queue.link(),
                            //    );
                            //}
                        }
					});

                if let Some(to_pause) = queue_to_pause {
                    self.0.rack
                        .iter_mut()
                        .chain(self.0.core.iter_mut())
                        .find(|(q, _)| {
                            q.link().to == to_pause
                        })
                        .map(|(q, ref mut already_paused)| {
                            if !*already_paused {
                                *already_paused = true;
                                q.force_tx_next(Packet::Pause(id)).unwrap();
                            }
                        });
                }

                Ok(vec![])
            }
        }
    }
    
    fn exec(&mut self, time: Nanos, logger: Option<&slog::Logger>) -> Result<Vec<Box<Event>>> {
        // step all queues forward
        let id = self.0.id;
        let ingress_queues = &mut self.1;
        let ingress_queue_mapping = &mut self.2;
        let num_links = self.0.rack.len();
        let mut queue_to_resume: Option<u32> = None;
        let evs = self.0.rack.iter_mut().chain(self.0.core.iter_mut())
            .filter(|(q, _)| {
                q.is_active()
            })
            .filter_map(|(q, _)| {
                q.set_active(false);
                if let Some(pkt) = q.dequeue() {
                    if let Some(log) = logger {
                        debug!(log, "tx";
                            "time" => time,
                            "node" => id,
                            "packet" => ?pkt,
                        );
                    }

                    match pkt {
                        Packet::Data{..} | Packet::Ack{..} | Packet::Nack{..} => {
                            let ingress_queue = ingress_queue_mapping.remove(&pkt).unwrap();

                            let virtual_ingress_queue_occupancy = ingress_queues.entry(ingress_queue)
                                .and_modify(|occ| { *occ -= pkt.get_size_bytes() })
                                .or_insert_with(|| unreachable!());

                            let per_ingress_static_pfc_thresh = ((q.headroom() - q.link().pfc_resume_threshold()) as f64 / num_links as f64) as u32;
                            if *virtual_ingress_queue_occupancy < per_ingress_static_pfc_thresh {
                                queue_to_resume = Some(ingress_queue);
                            }

                            if let Some(log) = logger {
                                debug!(log, "dequeue";
                                    "headroom" => q.headroom(),
                                    "ingress-occupancy" => *virtual_ingress_queue_occupancy,
                                    "resume" => ?queue_to_resume,
                                    "resume-head" => q.link().pfc_resume_threshold(),
                                    "resume-thresh" => per_ingress_static_pfc_thresh,
                                );
                            }
                        }
                        _ => {}
                    };
                    
                    Some(
                        Box::new(
                            NodeTransmitEvent(q.link(), pkt)
                        ) as Box<Event>,
                    )
                } else {
                    None
                }
            })
            .collect::<Vec<Box<Event>>>();

        if let Some(to_resume) = queue_to_resume {
            self.0.rack
                .iter_mut()
                .chain(self.0.core.iter_mut())
                .find(|(q, _)| {
                    q.link().to == to_resume
                })
                .map(|(q, ref mut already_paused)| {
                    if *already_paused {
                        if let Some(log) = logger {
                            debug!(log, "tx";
                                "time" => time,
                                "node" => id,
                                "packet" => ?Packet::Resume(id),
                            );
                        }

                        *already_paused = false;
                        q.force_tx_next(Packet::Resume(id)).unwrap();
                    }
                });
        }

        Ok(evs)
    }

    fn reactivate(&mut self, l: Link) {
        self.0.reactivate(l)
    }

    fn is_active(&self) -> bool {
        self.0.is_active()
    }
}
