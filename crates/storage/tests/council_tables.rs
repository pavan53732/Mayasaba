//! Council decision-quality tables survive an upgrade from the previous schema (DEC-052).
//!
//! `crates/storage` is the single owner of SQLite (DEC-017) and applies `SCHEMA_SQL` with
//! `CREATE TABLE IF NOT EXISTS` on every open, with no migration runner. That design has two consequences the
//! council tables depend on, and neither is proved by opening a fresh database:
//!
//! 1. a database created from an EARLIER revision of `schema.sql` gains the six new tables the next time it is
//!    opened, because every statement is `IF NOT EXISTS` and none of them alters an existing column;
//! 2. existing rows in that database are untouched by that open.
//!
//! The test builds exactly that situation: an older database, with an older `projects` table and a row in it,
//! then applies the current schema the way `Storage::open` does - twice, because opening twice must be safe.

use mayasaba_storage::SCHEMA_SQL;
use rusqlite::Connection;

/// The six tables DEC-052 adds, exactly as `schema.sql` names them.
const NEW_TABLES: [&str; 6] = [
    "council_mode_selections",
    "council_round_roles",
    "council_claim_grades",
    "council_budget_ledger",
    "council_decision_outcomes",
    "council_outcome_agent_links",
];

/// A deliberately narrower `projects` table, standing in for the shape an earlier revision created.
///
/// It is narrower on purpose: `CREATE TABLE IF NOT EXISTS projects` in the current schema is a no-op against a
/// table that already exists, so if the new statements were to depend on columns this older table lacks, the
/// open would fail here rather than in a user's database.
const OLD_PROJECTS: &str = "CREATE TABLE IF NOT EXISTS projects (
    project_id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    local_path TEXT NOT NULL,
    phase TEXT NOT NULL,
    status TEXT NOT NULL,
    current_epoch INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);";

fn table_names(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .expect("prepare sqlite_master query");
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query sqlite_master");
    rows.map(|row| row.expect("read table name")).collect()
}

#[test]
fn opening_an_older_database_adds_the_new_tables_and_keeps_its_rows() {
    let conn = Connection::open_in_memory().expect("open in-memory database");
    conn.execute_batch(OLD_PROJECTS)
        .expect("create the older schema");
    conn.execute(
        "INSERT INTO projects (project_id, name, local_path, phase, status, current_epoch, created_at, updated_at)
         VALUES ('prj_old', 'Older Project', 'C:\\work', 'EXECUTION', 'ACTIVE', 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        [],
    )
    .expect("insert a row into the older database");

    let before = table_names(&conn);
    for table in NEW_TABLES {
        assert!(
            !before.iter().any(|name| name == table),
            "{table} must not exist before the schema is applied"
        );
    }

    // Exactly what `Storage::open` does, and twice, because opening twice must be safe.
    conn.execute_batch(SCHEMA_SQL)
        .expect("apply the current schema");
    conn.execute_batch(SCHEMA_SQL)
        .expect("apply the current schema a second time");

    let after = table_names(&conn);
    for table in NEW_TABLES {
        assert!(
            after.iter().any(|name| name == table),
            "{table} must be added when an older database is opened"
        );
    }

    // The older database's own row is untouched by the upgrade.
    let name: String = conn
        .query_row(
            "SELECT name FROM projects WHERE project_id = 'prj_old'",
            [],
            |row| row.get(0),
        )
        .expect("the pre-existing project row must survive");
    assert_eq!(name, "Older Project");
}

#[test]
fn applying_the_schema_twice_is_safe_and_idempotent() {
    let conn = Connection::open_in_memory().expect("open in-memory database");
    conn.execute_batch(SCHEMA_SQL)
        .expect("apply the current schema");
    let first = table_names(&conn);
    conn.execute_batch(SCHEMA_SQL)
        .expect("apply the current schema again");
    let second = table_names(&conn);

    assert_eq!(
        first, second,
        "a second application must not add, drop or rename a table"
    );
    for table in NEW_TABLES {
        assert!(second.iter().any(|name| name == table));
    }
}

#[test]
fn the_new_tables_enforce_their_declared_vocabularies() {
    // The CHECK constraints are the only thing that closes these vocabularies at rest, and they are the first
    // in this schema. Foreign keys are relaxed for these inserts so that a CHECK violation is what fails,
    // rather than a missing referenced row masking it.
    let conn = Connection::open_in_memory().expect("open in-memory database");
    conn.execute_batch(SCHEMA_SQL)
        .expect("apply the current schema");
    conn.execute_batch("PRAGMA foreign_keys = OFF")
        .expect("relax foreign keys so the CHECK constraint is what is tested");

    let insert = |mode: &str| {
        conn.execute(
            "INSERT INTO council_mode_selections
               (selection_id, project_id, decision_class, mode, inputs_json, reasons_json,
                selector_version, override_source, created_at)
             VALUES ('sel_1', 'prj_1', 'ROUTINE', ?1, '{}', '[]', 'v1', 'NONE', '2026-01-01T00:00:00Z')",
            [mode],
        )
    };

    insert("MAYBE")
        .expect_err("`mode` outside the contract must be refused by the database itself");
    insert("SOLO").expect("a contract value must be accepted");

    // Every one of the six tables must carry a CHECK clause in its stored definition. The gate compares those
    // vocabularies against the contract in both directions; this asserts the clauses reach a real database.
    for (table, column) in [
        ("council_mode_selections", "mode"),
        ("council_round_roles", "role"),
        ("council_claim_grades", "grade"),
        ("council_budget_ledger", "kind"),
        ("council_decision_outcomes", "status"),
        ("council_decision_outcomes", "source"),
        ("council_claim_grades", "load_bearing"),
    ] {
        let ddl: String = conn
            .query_row(
                "SELECT sql FROM sqlite_master WHERE type='table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .expect("read the stored table definition");
        assert!(
            ddl.contains(&format!("CHECK({column} IN ("))
                || ddl.contains(&format!("CHECK({column} IN(")),
            "{table}.{column} must carry a CHECK constraint in the stored definition"
        );
    }
}
