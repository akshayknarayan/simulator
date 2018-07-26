use std::vec::Vec;

use slog;

use ::{Nanos, Result};
use event::Event;
use node::{NodeTransmitEvent, Link};
use packet::Packet;
use super::{Switch, Queue};

#[derive(Default, Debug)]
pub struct PFCSwitch {
    pub id: u32,
    pub active: bool,
    pub rack: Vec<(Box<Queue>, bool)>, // a queue to send, and whether we have paused the corresponding incoming queue 
    pub core: Vec<(Box<Queue>, bool)>, // a queue to send, and whether we have paused the corresponding incoming queue 
}

impl PFCSwitch {
    fn pause_incoming(&mut self, logger: Option<&slog::Logger>) {
        let id = self.id;
        // send pauses to upstream queues
        if let Some(log) = logger {
            debug!(log, "pausing";
               "node" => self.id,
            );
        }

        self.rack
            .iter_mut()
            .chain(self.core.iter_mut())
        .for_each(|(q, _)| {
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
            q.force_tx_next(Packet::Pause(id)).unwrap();
        });
    }

    fn resume_incoming(&mut self, logger: Option<&slog::Logger>) {
        let id = self.id;
        if let Some(log) = logger {
            debug!(log, "resuming";
                "node" => self.id,
            );
        }

        self.rack
            .iter_mut()
            .chain(self.core.iter_mut())
            .for_each(|(q, _)| {
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

    fn receive(&mut self, p: Packet, time: Nanos, logger: Option<&slog::Logger>) -> Result<Vec<Box<Event>>> {
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
					.map_or_else(|| unimplemented!(), |(rack_link_queue, ref mut already_paused)| {
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

                        if rack_link_queue.link().pfc_enabled
                            && !*already_paused 
                            && rack_link_queue.headroom() <= rack_link_queue.link().pfc_pause_threshold() {
                            // outgoing queue has filled up
                            *already_paused = true;
                            should_pause = true;
                        }
					});
                
                if should_pause {
                    self.pause_incoming(logger);
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
            .filter_map(|(q, ref mut is_paused)| {
                q.set_active(false);
                if let Some(pkt) = q.dequeue() {
                    // check if queue is sufficiently empty
                    if q.link().pfc_enabled
                        && *is_paused 
                        && q.headroom() > q.link().pfc_resume_threshold() {
                        *is_paused = false;
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
            self.resume_incoming(logger);
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
