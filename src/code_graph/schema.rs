//! SQLite schema cho Code Knowledge Graph.

use anyhow::Result;
use rusqlite::Connection;

pub fn apply_code_graph_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(r#"
        -- Symbols (Nodes): functions, classes, structs, traits, interfaces...
        CREATE TABLE IF NOT EXISTS cg_symbols (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id  TEXT    NOT NULL,
            file_path   TEXT    NOT NULL,
            name        TEXT    NOT NULL,
            kind        TEXT    NOT NULL,   -- 'function'|'class'|'struct'|'trait'|...
            signature   TEXT,
            start_line  INTEGER NOT NULL DEFAULT 0,
            end_line    INTEGER NOT NULL DEFAULT 0,
            language    TEXT    NOT NULL DEFAULT 'unknown',
            indexed_at  INTEGER NOT NULL    -- Unix ms
        );
        CREATE INDEX IF NOT EXISTS idx_cg_sym_project
            ON cg_symbols(project_id);
        CREATE INDEX IF NOT EXISTS idx_cg_sym_name
            ON cg_symbols(project_id, name);
        CREATE INDEX IF NOT EXISTS idx_cg_sym_file
            ON cg_symbols(project_id, file_path);

        -- Edges (Relationships): CALLS, IMPORTS, EXTENDS, IMPLEMENTS, DEFINES
        CREATE TABLE IF NOT EXISTS cg_edges (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id  TEXT    NOT NULL,
            from_sym_id INTEGER REFERENCES cg_symbols(id) ON DELETE CASCADE,
            from_name   TEXT    NOT NULL,
            from_file   TEXT    NOT NULL,
            to_sym_id   INTEGER REFERENCES cg_symbols(id) ON DELETE CASCADE,
            to_name     TEXT    NOT NULL,
            to_file     TEXT,
            kind        TEXT    NOT NULL,   -- 'calls'|'imports'|'extends'|'implements'|'defines'
            at_line     INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_cg_edge_from
            ON cg_edges(from_sym_id);
        CREATE INDEX IF NOT EXISTS idx_cg_edge_to_name
            ON cg_edges(project_id, to_name);
        CREATE INDEX IF NOT EXISTS idx_cg_edge_kind
            ON cg_edges(project_id, kind);

        -- Index state: track mtime per file to enable incremental re-indexing
        CREATE TABLE IF NOT EXISTS cg_index_state (
            project_id   TEXT    NOT NULL,
            file_path    TEXT    NOT NULL,
            mtime_secs   INTEGER NOT NULL,
            symbol_count INTEGER NOT NULL DEFAULT 0,
            edge_count   INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (project_id, file_path)
        );

        -- FTS5 index for full-text symbol search
        CREATE VIRTUAL TABLE IF NOT EXISTS cg_symbols_fts USING fts5(
            name,
            signature,
            file_path,
            kind,
            content='cg_symbols',
            content_rowid='id'
        );

        -- Triggers to keep FTS5 in sync
        CREATE TRIGGER IF NOT EXISTS cg_sym_ai AFTER INSERT ON cg_symbols BEGIN
            INSERT INTO cg_symbols_fts(rowid, name, signature, file_path, kind)
            VALUES (new.id, new.name, COALESCE(new.signature,''), new.file_path, new.kind);
        END;

        CREATE TRIGGER IF NOT EXISTS cg_sym_ad AFTER DELETE ON cg_symbols BEGIN
            INSERT INTO cg_symbols_fts(cg_symbols_fts, rowid, name, signature, file_path, kind)
            VALUES ('delete', old.id, old.name, COALESCE(old.signature,''), old.file_path, old.kind);
        END;
    "#)?;
    Ok(())
}
