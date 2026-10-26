PRAGMA application_id = 1129665603;
PRAGMA user_version = 1;
CREATE TABLE catalog (id INTEGER PRIMARY KEY CHECK(id=1), material_root TEXT NOT NULL);
CREATE TABLE materials (
    id INTEGER PRIMARY KEY, source_path TEXT NOT NULL UNIQUE,
    fingerprint TEXT NOT NULL, size_bytes INTEGER NOT NULL CHECK(size_bytes>=0),
    available INTEGER NOT NULL DEFAULT 1 CHECK(available IN (0,1)),
    needs_analysis INTEGER NOT NULL DEFAULT 1 CHECK(needs_analysis IN (0,1)),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE INDEX materials_analysis ON materials(available, needs_analysis);
CREATE TABLE analysis_jobs (
    id INTEGER PRIMARY KEY, material_id INTEGER NOT NULL REFERENCES materials(id),
    input_fingerprint TEXT NOT NULL, model TEXT NOT NULL, version TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('running','completed','failed','superseded')),
    error TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    finished_at TEXT
);
CREATE UNIQUE INDEX jobs_running ON analysis_jobs(material_id) WHERE status='running';
CREATE INDEX jobs_input ON analysis_jobs(material_id,input_fingerprint,model,version);
CREATE TABLE segments (
    id INTEGER PRIMARY KEY, material_id INTEGER NOT NULL REFERENCES materials(id),
    start_ms INTEGER NOT NULL CHECK(start_ms>=0), end_ms INTEGER NOT NULL CHECK(end_ms>start_ms),
    input_fingerprint TEXT NOT NULL,
    active INTEGER NOT NULL DEFAULT 1 CHECK(active IN (0,1)),
    ai_json TEXT NOT NULL DEFAULT '{}', manual_json TEXT NOT NULL DEFAULT '{}',
    effective_json TEXT NOT NULL DEFAULT '{}', search_text TEXT NOT NULL DEFAULT '',
    job_id INTEGER REFERENCES analysis_jobs(id),
    manual_updated_at TEXT,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    UNIQUE(material_id,start_ms,end_ms)
);
CREATE INDEX segments_material ON segments(material_id,active);
CREATE TABLE analysis_results (
    job_id INTEGER NOT NULL REFERENCES analysis_jobs(id),
    segment_id INTEGER NOT NULL REFERENCES segments(id),
    start_ms INTEGER NOT NULL CHECK(start_ms>=0), end_ms INTEGER NOT NULL CHECK(end_ms>start_ms),
    metadata_json TEXT NOT NULL, confidence REAL NOT NULL CHECK(confidence BETWEEN 0 AND 1),
    evidence TEXT NOT NULL, PRIMARY KEY(job_id,segment_id)
);
CREATE TABLE segment_attributes (
    segment_id INTEGER NOT NULL REFERENCES segments(id),
    kind TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY(segment_id,kind,value)
);
CREATE INDEX attributes_filter ON segment_attributes(kind,value,segment_id);
CREATE VIRTUAL TABLE segment_fts USING fts5(search_text, content='segments', content_rowid='id', tokenize='trigram');
CREATE TRIGGER segments_insert AFTER INSERT ON segments BEGIN
    INSERT INTO segment_fts(rowid,search_text) VALUES(new.id,new.search_text);
END;
CREATE TRIGGER segments_update AFTER UPDATE OF search_text ON segments BEGIN
    INSERT INTO segment_fts(segment_fts,rowid,search_text) VALUES('delete',old.id,old.search_text);
    INSERT INTO segment_fts(rowid,search_text) VALUES(new.id,new.search_text);
END;
CREATE TRIGGER segments_delete AFTER DELETE ON segments BEGIN
    INSERT INTO segment_fts(segment_fts,rowid,search_text) VALUES('delete',old.id,old.search_text);
END;
