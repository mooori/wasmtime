use std::fs;

const DATA_FILE_NAME: &str = "dummy.json";

fn main() {
    println!("Hello, world!");
    let inst_trace = InstTrace::new_from_file(DATA_FILE_NAME);
    println!("{:#?}", inst_trace);
}

#[derive(serde_derive::Deserialize, Debug)]
struct InstTrace {
    counters: Vec<InstCount>,
}

impl InstTrace {
    fn new_from_file(path: &str) -> anyhow::Result<Self> {
        let raw_data = fs::read_to_string(path)?;
        let trace: Self = serde_json::from_str(&raw_data)?;
        Ok(trace)
    }
}

#[derive(serde_derive::Deserialize, Debug)]
struct InstCount {
    /// Name of the instruction.
    name: String,
    /// Absolute number of executions.
    count: u64,
    /// Represents `count` divided by the total number of executed instructions.
    fraction: String,
}
