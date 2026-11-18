CREATE TABLE lines (
  id TEXT PRIMARY KEY,
  profile_id TEXT NOT NULL REFERENCES profiles(id) ON DELETE CASCADE,
  session_id TEXT NOT NULL,
  source TEXT NOT NULL CHECK (source IN ('subtitle', 'audio')),
  source_text TEXT NOT NULL,
  target_text TEXT NOT NULL,
  engine TEXT NOT NULL,
  is_fallback INTEGER NOT NULL DEFAULT 0,
  model_id TEXT,
  ocr_ms INTEGER,
  translate_ms INTEGER,
  created_at INTEGER NOT NULL
);
CREATE INDEX lines_profile_created ON lines (profile_id, created_at);
CREATE INDEX lines_session_created ON lines (session_id, created_at);
