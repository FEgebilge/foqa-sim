//! A recorder process killed without warning (TerminateProcess on Windows,
//! SIGKILL elsewhere: `kill -9`) must leave a journal that reads back cleanly
//! up to the last flushed packet.

use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::{env, thread};

use fdm_core::{Lfl, Sample, SignalId, SimTime};
use recorder::{JournalReader, JournalWriter, Record};

const PATH_VAR: &str = "FOQA_KILL_TEST_JOURNAL";

/// Child side: writes and flushes samples forever, until killed.
#[test]
#[ignore = "runs only as the child of killed_writer_leaves_a_readable_journal"]
fn writer_until_killed() {
    let Some(path) = env::var_os(PATH_VAR) else {
        return;
    };
    let mut journal =
        JournalWriter::new(BufWriter::new(File::create(path).unwrap()), &Lfl::V1).unwrap();
    let mut i = 0;
    loop {
        journal
            .sample(&Sample::new(SimTime::from_micros(i), SignalId(7), i as f64))
            .unwrap();
        journal.flush().unwrap();
        i += 1;
    }
}

#[test]
fn killed_writer_leaves_a_readable_journal() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("kill.fqj");
    let mut child = Command::new(env::current_exe().unwrap())
        .args(["writer_until_killed", "--exact", "--ignored"])
        .env(PATH_VAR, &path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    while fs::metadata(&path).map_or(0, |m| m.len()) < 100_000 {
        assert!(Instant::now() < deadline, "child never wrote");
        thread::sleep(Duration::from_millis(10));
    }
    child.kill().unwrap();
    child.wait().unwrap();

    let mut reader = JournalReader::new(BufReader::new(File::open(&path).unwrap())).unwrap();
    let mut count = 0;
    for record in reader.by_ref() {
        let Record::Sample(s) = record.unwrap() else {
            panic!("unexpected event")
        };
        assert_eq!(s.value, count as f64, "sample {count} out of sequence");
        count += 1;
    }
    drop(reader);
    assert!(count > 3000, "only {count} samples survived");
}
