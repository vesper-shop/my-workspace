use super::*;
use std::sync::Arc;

#[derive(Default)]
struct Memory {
    encoded: Option<String>,
    reads: usize,
    writes: usize,
    fail_read: bool,
    fail_write: bool,
}

#[derive(Clone, Default)]
struct TestBackend(Arc<Mutex<Memory>>);

impl Backend for TestBackend {
    fn read(&self) -> Result<Stored<String>, Error> {
        let mut memory = self.0.lock().unwrap();
        memory.reads += 1;
        if memory.fail_read {
            return Err(Error::StoreSynchronization);
        }
        Ok(match &memory.encoded {
            Some(value) => Stored::Ready(value.clone()),
            None => Stored::Missing,
        })
    }

    fn save(&self, value: &str) -> Result<(), Error> {
        let mut memory = self.0.lock().unwrap();
        if memory.fail_write {
            return Err(Error::StoreSynchronization);
        }
        memory.writes += 1;
        memory.encoded = Some(value.to_owned());
        Ok(())
    }
}

fn store(backend: &TestBackend) -> CredentialStore<TestBackend> {
    CredentialStore {
        backend: backend.clone(),
        cache: None,
    }
}

#[test]
fn single_keychain_read() {
    let backend = TestBackend::default();
    backend.0.lock().unwrap().encoded =
        Some(r#"{"entries":{"vesper":"session","nowledge":"key"}}"#.to_owned());
    let mut store = store(&backend);
    assert!(matches!(store.read("vesper").unwrap(), Stored::Ready(value) if value == "session"));
    assert!(matches!(store.read("nowledge").unwrap(), Stored::Ready(value) if value == "key"));
    assert!(matches!(store.read("ugos").unwrap(), Stored::Missing));
    store.save("vesper", Some("session")).unwrap();
    store.save("ugos", None).unwrap();
    let memory = backend.0.lock().unwrap();
    assert_eq!(memory.reads, 1);
    assert_eq!(memory.writes, 0);
}

#[test]
fn failed_writes() {
    let backend = TestBackend::default();
    let mut store = store(&backend);
    store.save("app-lock", Some("original")).unwrap();
    backend.0.lock().unwrap().fail_write = true;
    assert!(store.save("app-lock", Some("replacement")).is_err());
    assert!(matches!(store.read("app-lock").unwrap(), Stored::Ready(value) if value == "original"));
}

#[test]
fn invalid_credentials() {
    let backend = TestBackend::default();
    let mut store = store(&backend);
    backend.0.lock().unwrap().fail_read = true;
    assert!(store.save("app-lock", Some("new-password")).is_err());
    backend.0.lock().unwrap().fail_read = false;
    backend.0.lock().unwrap().encoded = Some(r#""private-value""#.to_owned());
    assert!(matches!(store.read("app-lock"), Err(Error::InvalidStore)));
    assert_eq!(backend.0.lock().unwrap().writes, 0);
}

#[test]
fn deletion() {
    let backend = TestBackend::default();
    let mut original = store(&backend);
    original.save("app-lock", Some("password")).unwrap();
    original.save("vesper", Some("session")).unwrap();
    original.save("app-lock", None).unwrap();
    let mut restarted = store(&backend);
    assert!(matches!(
        restarted.read("app-lock").unwrap(),
        Stored::Missing
    ));
    assert!(
        matches!(restarted.read("vesper").unwrap(), Stored::Ready(value) if value == "session")
    );
}

#[test]
fn concurrent_writers() {
    let backend = TestBackend::default();
    let shared = Mutex::new(store(&backend));
    std::thread::scope(|scope| {
        for index in 0..8 {
            let shared = &shared;
            scope.spawn(move || {
                shared
                    .lock()
                    .unwrap()
                    .save(
                        &format!("provider-{index}"),
                        Some(&format!("value-{index}")),
                    )
                    .unwrap();
            });
        }
    });
    let mut restarted = store(&backend);
    for index in 0..8 {
        assert!(
            matches!(restarted.read(&format!("provider-{index}")).unwrap(), Stored::Ready(value) if value == format!("value-{index}"))
        );
    }
    assert_eq!(backend.0.lock().unwrap().writes, 8);
}
