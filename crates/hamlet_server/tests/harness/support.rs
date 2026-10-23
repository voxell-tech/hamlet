//! A scratch home with a seeded realm, the driver process holding it,
//! and read-only looks at both stores.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use hamlet_server::{
    Actor, Admit, Change, End, FoundRecord, Hamlet, Limits,
    OperationId, RecordOperation, Reply, RoadBudget, RuntimeInstance,
    Store,
};
use hamlet_testing::{Scratch, strip_overrides};
use rusqlite::{Connection, OpenFlags};
use rustix::process::{Pid, Signal, kill_process, test_kill_process};
use serde_json::{Value, json};

/// How long a step may take before the test fails.
const PATIENCE: Duration = Duration::from_secs(30);

/// The price of a road and the cap on the hamlet's road spend, in
/// micro-dollars: two roads fit, a third does not.
pub const ROAD: u64 = 7;
pub const CAP: u64 = 19;

/// A scratch folder holding a home whose hamlet `home` has the mayor
/// `founder` (0) and the agents `ana` (1), `bo` (2) and `cy` (3).
pub struct Realm {
    pub scratch: Scratch,
    pub home: PathBuf,
}

impl Realm {
    pub fn new(tag: &str) -> Self {
        let scratch = Scratch::new(&format!("harness-{tag}"));
        let home = scratch.join("home");
        std::fs::create_dir_all(&home).unwrap();
        let mut store =
            Store::open(&home.join("realm.sqlite")).unwrap();
        let mut key = 0;
        let mut run = |store: &mut Store, change: Change| {
            key += 1;
            let request = RecordOperation {
                hamlet: "home".into(),
                actor: Actor::Chief("chief".into()),
                at: 1,
                reason: "Seeded".into(),
                task: None,
                change,
            };
            let id = OperationId {
                runtime: RuntimeInstance::new("seed").unwrap(),
                key: key.to_string(),
            };
            assert!(matches!(
                store.run(&id, &request).unwrap(),
                Reply::Outcome(hamlet_server::Outcome::Done(_))
            ));
        };
        let found = FoundRecord {
            hamlet: "home".into(),
            name: "founder".into(),
            purpose: "Leads".into(),
            chief: "chief".into(),
            at: 0,
        };
        let id = OperationId {
            runtime: RuntimeInstance::new("seed").unwrap(),
            key: "found".into(),
        };
        store.run(&id, &found).unwrap();
        for name in ["ana", "bo", "cy"] {
            run(
                &mut store,
                Change::Admit(Admit {
                    name: name.into(),
                    purpose: "Works".into(),
                    model: None,
                    character: String::new(),
                    roads: Vec::new(),
                }),
            );
        }
        let mut limits = Limits::default();
        limits.chief.road.cost = ROAD as f64 / 1_000_000.0;
        limits.road_budget = Some(RoadBudget {
            start: 0,
            end: i64::MAX,
            month: false,
            other: limits.chief.road_price(),
            cost: Some(CAP),
            ms: None,
        });
        limits.road_budget.as_mut().unwrap().other.micros = 0;
        run(
            &mut store,
            Change::SetLimits {
                limits,
                runs: Vec::new(),
            },
        );
        store.close().unwrap();
        Self { scratch, home }
    }

    /// Writes the faux script `script` and returns its path.
    pub fn script(&self, name: &str, script: Value) -> PathBuf {
        let path = self.scratch.join(name);
        std::fs::write(&path, script.to_string()).unwrap();
        path
    }

    /// Saves settings that run `model` on the endpoint `endpoint`,
    /// served at `url`.
    pub fn serve(&self, model: &str, url: &str) {
        let (endpoint, _) = model.split_once("::").unwrap();
        let settings = json!({
            "model": model, "reflectEvery": 10, "allowNet": false,
            "endpoints": [{"name": endpoint, "baseUrl": url}],
            "link": {"state": "untested", "models": null, "words": null},
            "saved": true,
        });
        Connection::open(self.home.join("realm.sqlite"))
            .unwrap()
            .execute(
                "INSERT INTO chief_document(kind, key, value) \
                 VALUES ('settings', 'home', ?1)",
                [settings.to_string()],
            )
            .unwrap();
    }

    /// The folder where the processes' faults leave their marks.
    pub fn faults(&self) -> PathBuf {
        let dir = self.scratch.join("faults");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub fn paused(&self, process: &str) -> Option<String> {
        // An empty file is a pause still being written.
        std::fs::read_to_string(
            self.faults().join(format!("{process}-paused")),
        )
        .ok()
        .filter(|point| !point.is_empty())
    }

    /// Waits until the process `process` (`server` or `runtime`)
    /// stopped at a fault point; returns the point.
    pub fn wait_paused(&self, process: &str) -> String {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(point) = self.paused(process) {
                return point;
            }
            assert!(
                Instant::now() < deadline,
                "{process} never paused"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The realm as its database holds it now.
    pub fn look(&self) -> Look {
        let db = Connection::open_with_flags(
            self.home.join("realm.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let document = db
            .query_row(
                "SELECT document FROM record_hamlet WHERE id = \
                 'home'",
                [],
                |row| row.get::<_, String>(0),
            )
            .unwrap();
        let hamlet =
            serde_json::from_str::<Hamlet>(&document).unwrap();
        let name = |end: End| match end {
            End::Office => "office".to_string(),
            End::Agent(number) => hamlet
                .agents
                .iter()
                .find(|agent| agent.number == number)
                .map(|agent| agent.name.clone())
                .unwrap_or_default(),
        };
        let mut roads = hamlet
            .roads
            .iter()
            .map(|road| {
                let mut ends = [name(road.a), name(road.b)];
                ends.sort();
                (ends[0].clone(), ends[1].clone())
            })
            .collect::<Vec<_>>();
        roads.sort();
        let all = |sql: &str| {
            let mut query = db.prepare(sql).unwrap();
            query
                .query_map([], |row| {
                    let columns = row.as_ref().column_count();
                    (0..columns)
                        .map(|i| {
                            Ok(match row.get_ref(i)? {
                                rusqlite::types::ValueRef::Integer(n) => {
                                    json!(n)
                                }
                                rusqlite::types::ValueRef::Text(text) => {
                                    json!(String::from_utf8_lossy(text))
                                }
                                rusqlite::types::ValueRef::Null => Value::Null,
                                _ => json!("?"),
                            })
                        })
                        .collect::<rusqlite::Result<Vec<_>>>()
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap()
        };
        Look {
            roads,
            charges: all(
                "SELECT micros FROM record_road_charge ORDER BY id"
            )
            .into_iter()
            .map(|row| row[0].as_u64().unwrap())
            .collect(),
            markers: all("SELECT key, outcome FROM marker WHERE \
                          runtime != 'seed' ORDER BY key"),
            waits: all("SELECT g.key, CASE WHEN w.open THEN \
                        'pending' ELSE w.how END FROM gate_call g \
                        JOIN wait w ON w.id = g.wait ORDER BY g.key"),
            inputs: all("SELECT request, state FROM harness_input \
                         ORDER BY ordinal"),
            attempts: all("SELECT key FROM harness_attempt").len(),
            usage: all("SELECT key, micros FROM harness_usage").len(),
            micros: all("SELECT micros FROM harness_usage")
                .into_iter()
                .map(|row| row[0].as_u64().unwrap())
                .sum(),
        }
    }

    /// Reserves building roads to the chief, so the mayor's road
    /// calls wait for a chief's answer.
    pub fn reserve_roads(&self) {
        let db =
            Connection::open(self.home.join("realm.sqlite")).unwrap();
        db.execute(
            "UPDATE record_hamlet SET document = json_set(document, \
             '$.limits.chief.mayor.buildRoads', json('false')) \
             WHERE id = 'home'",
            [],
        )
        .unwrap();
    }

    /// The store of the runtime `runtime`, read-only, if it was made.
    fn pi(&self, runtime: &str) -> Option<Connection> {
        let path = self
            .home
            .join("harnesses")
            .join(runtime)
            .join("pi.sqlite");
        path.exists().then(|| {
            Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap()
        })
    }

    /// How many inputs pi admitted in the runtime `runtime`.
    pub fn submissions(&self, runtime: &str) -> u32 {
        self.pi(runtime).map_or(0, |db| {
            db.query_row(
                "SELECT count(*) FROM submissions",
                [],
                |row| row.get::<_, u32>(0),
            )
            .unwrap()
        })
    }

    /// The states of the inputs pi admitted in the runtime `runtime`.
    pub fn submission_states(&self, runtime: &str) -> Vec<String> {
        let Some(db) = self.pi(runtime) else {
            return Vec::new();
        };
        let mut query = db
            .prepare("SELECT status FROM submissions ORDER BY id")
            .unwrap();
        query
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    /// The tool results the runtime `runtime` kept, read from its
    /// store.
    pub fn kept_tool_results(&self, runtime: &str) -> Vec<Value> {
        let Some(db) = self.pi(runtime) else {
            return Vec::new();
        };
        let mut query =
            db.prepare("SELECT record FROM entries").unwrap();
        query
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(|record| {
                serde_json::from_str::<Value>(&record.unwrap())
                    .unwrap()
            })
            .flat_map(|entry| {
                entry["model"].as_array().cloned().unwrap_or_default()
            })
            .filter(|message| message["role"] == "toolResult")
            .collect()
    }
}

/// The realm's records, the gate's waits and the server's harness
/// tables at one moment.
#[derive(Debug)]
pub struct Look {
    pub roads: Vec<(String, String)>,
    pub charges: Vec<u64>,
    pub markers: Vec<Vec<Value>>,
    pub waits: Vec<Vec<Value>>,
    pub inputs: Vec<Vec<Value>>,
    pub attempts: usize,
    pub usage: usize,
    /// What the model calls cost, in micro-dollars.
    pub micros: u64,
}

impl Look {
    pub fn untouched(&self) -> bool {
        self.roads.is_empty()
            && self.charges.is_empty()
            && self.markers.is_empty()
    }
}

/// The driver process holding a home, with what it printed so far.
pub struct Driver {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: Receiver<Value>,
    pub notices: Vec<Value>,
    log: PathBuf,
}

impl Driver {
    /// Starts the driver on `realm` with `faux` and `env`, and waits
    /// until it is ready.
    pub fn start(
        realm: &Realm,
        faux: &Path,
        env: &[(&str, &str)],
    ) -> Self {
        Self::start_on(realm, Some(faux), env)
    }

    /// Starts the driver on `realm` with `env`, and with `faux` when
    /// given.
    pub fn start_on(
        realm: &Realm,
        faux: Option<&Path>,
        env: &[(&str, &str)],
    ) -> Self {
        let log = realm.scratch.join("driver.log");
        let mut command =
            Command::new(env!("CARGO_BIN_EXE_hamlet-harness-driver"));
        command.arg("--home").arg(&realm.home);
        if let Some(faux) = faux {
            command.arg("--faux").arg(faux);
        }
        strip_overrides(&mut command)
            .env("HAMLET_FAULT_DIR", realm.faults())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log)
                    .unwrap(),
            );
        for (name, value) in env {
            command.env(name, value);
        }
        let mut child = command.spawn().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                let Ok(value) = serde_json::from_str::<Value>(&line)
                else {
                    continue;
                };
                if sender.send(value).is_err() {
                    return;
                }
            }
        });
        let stdin = child.stdin.take();
        let mut driver = Self {
            child,
            stdin,
            lines,
            notices: Vec::new(),
            log,
        };
        driver.reply_where(|line| line["ready"] == true);
        driver
    }

    /// What the driver and the runtimes printed on standard error.
    pub fn log(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn next_line(&mut self, deadline: Instant) -> Value {
        let left = deadline.saturating_duration_since(Instant::now());
        match self.lines.recv_timeout(left) {
            Ok(line) => line,
            Err(RecvTimeoutError::Timeout) => {
                panic!(
                    "timed out; notices {:#?}\n{}",
                    self.notices,
                    self.log()
                )
            }
            Err(RecvTimeoutError::Disconnected) => {
                panic!(
                    "the driver ended; notices {:#?}\n{}",
                    self.notices,
                    self.log()
                )
            }
        }
    }

    fn reply_where(
        &mut self,
        wanted: impl Fn(&Value) -> bool,
    ) -> Value {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let line = self.next_line(deadline);
            if line.get("notice").is_some() {
                self.notices.push(line["notice"].clone());
            } else if wanted(&line) {
                return line;
            } else if line.get("error").is_some() {
                panic!("{line}\n{}", self.log());
            }
        }
    }

    /// Sends a command and returns its reply.
    pub fn send(&mut self, command: Value) -> Value {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{command}").unwrap();
        stdin.flush().unwrap();
        self.reply_where(|line| line.get("notice").is_none())
    }

    /// Waits for the `count`th notice that `wanted` matches, counting
    /// every notice since the driver started; returns it.
    pub fn until_nth(
        &mut self,
        count: usize,
        wanted: impl Fn(&Value) -> bool,
    ) -> Value {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let found = self
                .notices
                .iter()
                .filter(|notice| wanted(notice))
                .nth(count - 1);
            if let Some(found) = found {
                return found.clone();
            }
            let line = self.next_line(deadline);
            if line.get("notice").is_some() {
                self.notices.push(line["notice"].clone());
            }
        }
    }

    pub fn until(
        &mut self,
        wanted: impl Fn(&Value) -> bool,
    ) -> Value {
        self.until_nth(1, wanted)
    }

    /// Waits for the final answer `text` of the conversation
    /// `binding`.
    pub fn answer(&mut self, binding: i64, text: &str) -> Value {
        self.until(|notice| {
            notice["kind"] == "kept"
                && notice["binding"] == binding
                && notice["item"]["kind"] == "assistant"
                && notice["item"]["text"] == text
        })
    }

    /// Binds a conversation for the agent `number` of `home`; returns
    /// its binding and runtime.
    pub fn bind(&mut self, number: u64) -> (i64, String) {
        let reply = self.send(
            json!({"do": "bind", "hamlet": "home", "number": number}),
        );
        (
            reply["bound"].as_i64().unwrap(),
            reply["runtime"].as_str().unwrap().to_string(),
        )
    }

    pub fn submit(
        &mut self,
        binding: i64,
        request: &str,
        content: &str,
    ) {
        self.send(json!({
            "do": "submit", "binding": binding, "request": request, "content": content,
        }));
    }

    /// The process id of the runtime most recently started.
    pub fn runtime_pid(&mut self) -> u32 {
        let started = self
            .notices
            .iter()
            .filter(|n| n["kind"] == "started")
            .count();
        let started = self
            .until_nth(started.max(1), |n| n["kind"] == "started");
        started["pid"].as_u64().unwrap() as u32
    }

    /// The whole transcript of `binding`, through the runtime's
    /// history.
    pub fn history(&mut self, binding: i64) -> Vec<Value> {
        let pages = self
            .notices
            .iter()
            .filter(|n| n["kind"] == "page")
            .count();
        self.send(json!({"do": "history", "binding": binding, "limit": 200}));
        let page = self.until_nth(pages + 1, |n| n["kind"] == "page");
        page["items"].as_array().cloned().unwrap()
    }

    /// Kills the driver at once, as a crash would.
    pub fn kill(mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }

    /// Ends the driver in order: its runtimes are told to go first.
    pub fn stop(mut self) {
        drop(self.stdin.take());
        let status = self.child.wait().unwrap();
        assert!(status.success(), "{}", self.log());
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn process(pid: u32) -> Pid {
    Pid::from_raw(pid as i32).expect("a process id")
}

/// Kills the process `pid` at once.
pub fn kill(pid: u32) {
    kill_process(process(pid), Signal::KILL)
        .unwrap_or_else(|_| panic!("no process {pid}"));
}

/// Whether the process `pid` still runs.
fn is_alive(pid: u32) -> bool {
    test_kill_process(process(pid)).is_ok()
}

/// Waits until the process `pid` is gone.
pub fn gone(pid: u32) {
    let deadline = Instant::now() + PATIENCE;
    while is_alive(pid) {
        assert!(
            Instant::now() < deadline,
            "process {pid} still runs"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
