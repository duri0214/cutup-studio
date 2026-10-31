PRAGMA application_id = 1129665603;
PRAGMA user_version = 2;
CREATE TABLE catalog (id INTEGER PRIMARY KEY CHECK(id=1), material_root TEXT NOT NULL);
CREATE TABLE materials (
    id INTEGER PRIMARY KEY,
    source_path TEXT NOT NULL UNIQUE,
    fingerprint TEXT NOT NULL,
    size_bytes INTEGER NOT NULL CHECK(size_bytes>=0),
    available INTEGER NOT NULL DEFAULT 1 CHECK(available IN (0,1)),
    metadata_fingerprint TEXT,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    search_text TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))
);
CREATE TABLE material_attributes (
    material_id INTEGER NOT NULL REFERENCES materials(id),
    kind TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY(material_id,kind,value)
);
CREATE INDEX attributes_filter ON material_attributes(kind,value,material_id);
CREATE VIRTUAL TABLE material_fts USING fts5(search_text, content='materials', content_rowid='id', tokenize='trigram');
CREATE TRIGGER materials_insert AFTER INSERT ON materials BEGIN
    INSERT INTO material_fts(rowid,search_text) VALUES(new.id,new.search_text);
END;
CREATE TRIGGER materials_update AFTER UPDATE OF search_text ON materials BEGIN
    INSERT INTO material_fts(material_fts,rowid,search_text) VALUES('delete',old.id,old.search_text);
    INSERT INTO material_fts(rowid,search_text) VALUES(new.id,new.search_text);
END;
CREATE TRIGGER materials_delete AFTER DELETE ON materials BEGIN
    INSERT INTO material_fts(material_fts,rowid,search_text) VALUES('delete',old.id,old.search_text);
END;
