//! The run_code helper as the router starts it: `janus run-code-helper`, given a script and its
//! limits on stdin, asking for each tool call on stdout and answered on stdin. Being an
//! integration test, it also has `cargo test` build `target/debug/janus`, which the unit tests
//! that run a script end to end use as their helper (`coderun::test_exe`).

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn the_helper_asks_for_each_call_and_says_what_the_script_came_to() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_janus"))
        .arg("run-code-helper")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let start = r#"{"script":{"code":"const r = await tools.echo({x: 1}); return r.x + 1;","args":null,"tools":["echo"],"skills":[]},"limits":{"calls":100,"at_once":8,"max":40000,"heap":536870912,"wait":5000}}"#;
    writeln!(stdin, "{start}").unwrap();
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    let said: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(said, json!({ "call": { "id": 1, "name": "echo", "args": { "x": 1 } } }));
    writeln!(stdin, r#"{{"id":1,"res":{{"content":[{{"type":"text","text":"{{\"x\": 1}}"}}]}}}}"#).unwrap();
    line.clear();
    out.read_line(&mut line).unwrap();
    let said: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(said, json!({ "done": { "content": [{ "type": "text", "text": "2" }], "isError": false } }));
    assert!(child.wait().unwrap().success());
}
