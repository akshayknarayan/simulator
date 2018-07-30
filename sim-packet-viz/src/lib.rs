use std::io::{BufRead, BufReader};
use std::collections::VecDeque;
use std::fmt;

#[macro_use] extern crate failure;
#[macro_use] extern crate lazy_static;
extern crate json;
extern crate regex;
use regex::Regex;

#[derive(Debug)]
pub enum EventColor {
    Black,
    Blue,
    Red,
    Green,
}

impl std::fmt::Display for EventColor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", match self {
            EventColor::Black => "black",
            EventColor::Blue => "blue",
            EventColor::Red => "red",
            EventColor::Green => "green",
        })
    }
}

#[derive(Debug)]
pub struct EventMatch(Option<Box<LogEvent>>, Option<Box<LogEvent>>);

#[derive(Debug, PartialEq)]
pub enum EventMatchSide {
    Tx,
    Rx,
}

pub trait LogEvent: std::fmt::Debug {
    fn adj_time(&mut self, start_time: usize);
    fn time(&self) -> usize;
    fn from(&self) -> usize;
    fn to(&self) -> usize;
    fn node(&self) -> usize;
    fn event(&self) -> Option<EventMatchSide>;
    fn annotation(&self) -> String;
    fn color(&self) -> EventColor;
}

pub struct SlogJSONReader<R: std::io::Read>(R);

#[derive(Debug)]
pub struct JsonLogEvent{
    json_object: json::JsonValue, 
    adj_time: usize,
    packet_type: String, 
    flow: usize,
    from: usize, 
    to: usize, 
    seq: usize,
}

impl JsonLogEvent {
    fn new(json: json::JsonValue) -> Result<Self, failure::Error> {
        lazy_static! {
            static ref TYPE_RE: Regex = Regex::new(r"^(\w+) ").unwrap();
            static ref FROM_RE: Regex = Regex::new(r" from: (\d+)(?:,| )").unwrap();
            static ref TO_RE: Regex = Regex::new(r" to: (\d+)(?:,| )").unwrap();
            static ref FLOW_RE: Regex = Regex::new(r" flow: (\d+)(?:,| )").unwrap();
            static ref SEQ_RE: Regex = Regex::new(r"seq: (\d+)(?:,| )").unwrap();
        }

        if !json.has_key("packet") {
            bail!("Packet field not found")
        } else {
            let j = json.clone();
            let pkt_line = j["packet"].as_str().unwrap();
            let type_match = TYPE_RE.captures(pkt_line).ok_or_else(|| format_err!("Did not match type"))?;
            let from_match = FROM_RE.captures(pkt_line).ok_or_else(|| format_err!("Did not match from"))?;
            let to_match = TO_RE.captures(pkt_line).ok_or_else(|| format_err!("Did not match to"))?;
            let flow_match = FLOW_RE.captures(pkt_line).ok_or_else(|| format_err!("Did not match flow"))?;
            let seq_match = SEQ_RE.captures(pkt_line).ok_or_else(|| format_err!("Did not match seq"))?;

            let adj_time = json["time"].as_usize().unwrap();

            Ok(JsonLogEvent{
                json_object: json,
                adj_time,
                packet_type: type_match[1].to_string(),
                flow: flow_match[1].parse()?,
                from: from_match[1].parse()?,
                to: to_match[1].parse()?,
                seq: seq_match[1].parse()?,
            })
        }
    }
}

impl LogEvent for JsonLogEvent {
    fn adj_time(&mut self, start_time: usize) {
        self.adj_time -= start_time;
    }

    fn time(&self) -> usize {
        self.adj_time
    }

    fn from(&self) -> usize {
        self.from
    }
    
    fn to(&self) -> usize {
        self.to
    }

    fn node(&self) -> usize {
        self.json_object["node"].as_usize().unwrap()
    }

    fn event(&self) -> Option<EventMatchSide> {
        match self.json_object["msg"].as_str().unwrap() {
            "tx" => Some(EventMatchSide::Tx),
            "rx" => Some(EventMatchSide::Rx),
            _ => None,
        }
    }

    fn annotation(&self) -> String {
        format!("{}-{}-{}", self.flow, self.packet_type, self.seq.to_string())
    }

    fn color(&self) -> EventColor {
        match self.packet_type.as_str() {
            "Data" => EventColor::Black,
            "Ack" => EventColor::Green,
            "Nack" => EventColor::Red,
            _ => EventColor::Blue,
        }
    }
}

impl<R: std::io::Read> SlogJSONReader<R> {
    pub fn new(r: R) -> Self {
        SlogJSONReader(r)
    }

    pub fn get_events(self) -> impl Iterator<Item=Box<LogEvent + 'static>> {
        let f = BufReader::new(self.0);
        let mut start_time: Option<usize> = None;
        f.lines()
            .take_while(|l| l.is_ok())
            .map(|l| l.unwrap())
            .filter(|l| !l.trim().is_empty())
            .filter_map(|line| {
                let parsed = json::parse(&line).ok()?;
                JsonLogEvent::new(parsed).ok()
            })
            .map(move |mut parsed| {
                let start = start_time.get_or_insert(parsed.time());
                parsed.adj_time(start.clone());
                Box::new(parsed) as Box<LogEvent>
            })
    }
}

pub trait VizWriter {
    fn dump_events(&mut self, events: impl Iterator<Item=Box<LogEvent>>) -> Result<(), failure::Error>;
}

pub struct TikzWriter<W: std::io::Write> {
    dump: W,
    nodes: Vec<(usize, usize)>,
}

impl<W: std::io::Write> TikzWriter<W> {
    pub fn new(w: W, nodes: &[(usize, usize)]) -> Self {
        TikzWriter { dump: w, nodes: nodes.to_vec() }
    }

    fn dump(&mut self, s: &str) -> Result<(), failure::Error> {
        self.dump.write(s.as_bytes()).map(|_| ()).map_err(failure::Error::from)
    }

    fn lookup(&self, node: usize) -> Option<usize> {
        self.nodes
            .iter()
            .find(|&(n, _)| *n == node)
            .map(|&(_, n)| n)
    }

    fn prelude(&mut self) -> Result<(), failure::Error> {
        let s = r#"
        \documentclass[class=minimal,border=5pt]{standalone}
        \usepackage{tikz}

        \begin{document}
        \begin{tikzpicture}
        "#;
        self.dump(s)
    }

    fn postlude(&mut self, end_time: usize) -> Result<(), failure::Error> {
        let nodes = self.nodes.iter();
        let dump = &mut self.dump;
        for node in nodes {
            let s = format!(
                r#"\draw[very thick] ({0}, 0) -- ({0}, -{1}) ;
                \draw ({0}, 0.5) node {{{2}}} ;
                "#, 
                node.1, 
                end_time as f64 / 1e6,
                node.0
            );

            dump.write(s.as_bytes()).map_err(failure::Error::from)?;
        }

        let s = r#"
        \end{tikzpicture}
        \end{document}
        "#;
        dump.write(s.as_bytes()).map(|_| ()).map_err(failure::Error::from)
    }

    fn single_edge(&mut self, tx_edge: &Box<LogEvent>, rx_edge: &Box<LogEvent>) -> Result<(), failure::Error> {
        // draw from (from node, tx time) -> (to node, rx time)
        let tx_time = tx_edge.time() as f64 / 1e6; // ms
        let rx_time = rx_edge.time() as f64 / 1e6; // ms

        // determine from_node
        let tx_node = tx_edge.node();
        let rx_node = rx_edge.node();

        if let None = self.lookup(tx_node) {
            return Ok(()); // skip
        }
        
        if let None = self.lookup(rx_node) {
            return Ok(()); // skip
        }

        let s = format!(
            r#"\draw[{4}] ({0},-{1}) -> ({2},-{3}) 
              node[pos=0.5,sloped,{4}] {{{5}}} ;
            "#, 
            self.lookup(tx_node).unwrap(), 
            tx_time, 
            self.lookup(rx_node).unwrap(), 
            rx_time,
            tx_edge.color(),
            tx_edge.annotation(),
        );
        self.dump(&s)
    }
}

impl<W: std::io::Write> VizWriter for TikzWriter<W> {
    fn dump_events(&mut self, events: impl Iterator<Item=Box<LogEvent>>) -> Result<(), failure::Error> {
        self.prelude()?;
        use std::collections::HashMap;
        let mut pending_edges: HashMap<String, VecDeque<Box<LogEvent>>> = HashMap::new();
        let mut end_time = 0;
        for ev in events {
            end_time = ev.time();
            match ev.event() {
                Some(EventMatchSide::Tx) => {
                    let val = pending_edges.entry(ev.annotation()).or_insert_with(|| VecDeque::new());
                    val.push_back(ev);
                }
                Some(EventMatchSide::Rx) => {
                    if let Some(tx) = pending_edges.get_mut(&ev.annotation()) {
                        match tx.pop_front() {
                            Some(tx) => self.single_edge(&tx, &ev)?,
                            None => bail!("Found unmatched tx: {:?}", ev.annotation()),
                        }
                    } else {
                        bail!("Found unmatched rx: {:?}", ev.annotation());
                    }
                }
                _ => continue,
            }
            
        }

        self.postlude(end_time)
    }
}

#[cfg(test)]
mod tests {
    use std;
    use super::{SlogJSONReader, EventMatchSide, LogEvent, VizWriter, TikzWriter};
    
    #[test]
    fn slog_json_parse() {
        let log_sample = r#"
        {"msg":"rx","v":0,"name":"slog-rs","level":20,"time":"2018-07-27T09:37:56.848190-07:00","hostname":"Y4089549","pid":6323,"packet":"Data { hdr: PacketHeader { flow: 0, from: 0, to: 1 }, seq: 37960, length: 1460 }","node":1,"time":1202560000}
        {"msg":"flow completed","v":0,"name":"slog-rs","level":30,"time":"2018-07-27T09:37:56.849518700-07:00","hostname":"Y4089549","pid":6323,"end_time":1207280000,"start_time":1126000000,"completion_time":81280000,"side":"Receiver","node":1,"flow":0}
        "#;
        let reader = std::io::BufReader::new(log_sample.as_bytes());
        let reader = SlogJSONReader(reader);
        let evs: Vec<Box<LogEvent>> = reader.get_events().collect();
        assert_eq!(evs.len(), 1);
        let ev = &evs[0];
        assert_eq!(ev.time(), 0);
        assert_eq!(ev.from(), 0);
        assert_eq!(ev.to(), 1);
        assert_eq!(ev.node(), 1);
        assert_eq!(ev.event(), Some(EventMatchSide::Rx));
        assert_eq!(ev.annotation(), "0-Data-37960");
    }
    
    #[test]
    fn nack_parse() {
        let log_sample = r#"
        {"msg":"rx","v":0,"name":"slog-rs","level":20,"time":"2018-07-27T09:37:56.848190-07:00","hostname":"Y4089549","pid":6323,"packet":"Nack { hdr: PacketHeader { flow: 0, from: 0, to: 1 }, seq: 37960, length: 1460 }","node":1,"time":1202560000}
        "#;
        let reader = std::io::BufReader::new(log_sample.as_bytes());
        let reader = SlogJSONReader(reader);
        let evs: Vec<Box<LogEvent>> = reader.get_events().collect();
        let ev = &evs[0];
        assert_eq!(ev.annotation(), "0-Nack-37960");
    }

    #[test]
    fn slog_json_tikz() {
        let log_sample = r#"
        {"msg":"tx","v":0,"name":"slog-rs","level":20,"time":"2018-07-27T09:37:56.844790100-07:00","hostname":"Y4089549","pid":6323,"packet":"Ack { hdr: PacketHeader { flow: 0, from: 1, to: 0 }, cumulative_acked_seq: 32120 }","node":1,"time":1191200000}
        {"msg":"rx","v":0,"name":"slog-rs","level":20,"time":"2018-07-27T09:37:56.845250600-07:00","hostname":"Y4089549","pid":6323,"packet":"Ack { hdr: PacketHeader { flow: 0, from: 1, to: 0 }, cumulative_acked_seq: 32120 }","node":4,"time":1192520000}
        {"msg":"tx","v":0,"name":"slog-rs","level":20,"time":"2018-07-27T09:37:56.845318700-07:00","hostname":"Y4089549","pid":6323,"packet":"Ack { hdr: PacketHeader { flow: 0, from: 1, to: 0 }, cumulative_acked_seq: 32120 }","node":4,"time":1192520000}
        {"msg":"rx","v":0,"name":"slog-rs","level":20,"time":"2018-07-27T09:37:56.845845800-07:00","hostname":"Y4089549","pid":6323,"packet":"Ack { hdr: PacketHeader { flow: 0, from: 1, to: 0 }, cumulative_acked_seq: 32120 }","node":0,"time":1193840000}
        "#;

        // parse
        let reader = std::io::BufReader::new(log_sample.as_bytes());
        let reader = SlogJSONReader(reader);

        // dump
        use std::io::Cursor;
        let mut buf = Cursor::new(vec![0;1024]);
        {
        let mut writer = TikzWriter::new(&mut buf, &[(0, 0), (4, 5), (1, 10)]);
        writer.dump_events(reader.get_events()).unwrap();
        }

        let res = buf.into_inner().into_iter().take_while(|&b| b != 0).collect::<Vec<u8>>();
        let _output = std::str::from_utf8(&res).unwrap();
    }
}
