CREATE TABLE analysis_jobs (
    material_id INTEGER PRIMARY KEY REFERENCES materials(id) ON DELETE CASCADE,
    fingerprint TEXT NOT NULL,
    configuration TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('running', 'completed', 'failed', 'interrupted')),
    candidates_json TEXT NOT NULL DEFAULT '[]',
    error TEXT
);
PRAGMA user_version = 4;
