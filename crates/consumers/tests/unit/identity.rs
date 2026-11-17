use super::SCOPES;

/// `SCOPES` in vesper-cloud's `packages/core/src/constants.ts`; authorize refuses any other.
const SERVER: [&str; 8] = [
    "entries:create",
    "entries:read",
    "entries:update",
    "entries:delete",
    "photos:create",
    "photos:read",
    "tags:read",
    "tags:update",
];

#[test]
fn requests_only_scopes_vesper_grants() {
    let requested: Vec<&str> = SCOPES.split(' ').collect();
    assert!(requested.contains(&"offline_access"));
    assert!(
        requested
            .iter()
            .filter(|scope| **scope != "offline_access")
            .all(|scope| SERVER.contains(scope))
    );
}
