use std::{
    collections::VecDeque,
    time::{Instant, SystemTime},
};

use crate::runner::CmdOutput;

pub struct Analyzer {
    pub samples: VecDeque<Sample>,
}

#[derive(Debug, Clone, Copy)]
pub struct Sample {
    // TODO: Customization
    pub instant: Instant,
    pub time: SystemTime,
    pub value: f64,
    pub max: f64,
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            samples: VecDeque::new(),
        }
    }

    pub fn process_output(&mut self, outp: &CmdOutput) {
        // TODO: Customize the detection rule
        lazy_static::lazy_static! {
            static ref RE: regex::Regex = regex::Regex::new("([0-9]+)/([0-9]+)").unwrap();
        }

        if let Some(mat) = RE.captures(&outp.stdout) {
            // TODO: Annotate the text with span information
            let instant = Instant::now();
            let time = SystemTime::now();
            let value: f64 = mat[1].parse().unwrap();
            let max: f64 = mat[2].parse().unwrap();
            self.samples.push_back(Sample {
                instant,
                time,
                value,
                max,
            });
        }

        if self.samples.len() > 1000 {
            self.samples.pop_front();
        }
    }
}
