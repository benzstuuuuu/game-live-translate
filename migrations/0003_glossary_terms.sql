-- SRS 7.2: profile_id '' is the shared glossary; a game's terms go with its profile.
CREATE TABLE glossary_terms (
  id TEXT PRIMARY KEY,
  profile_id TEXT NOT NULL DEFAULT '',
  source TEXT NOT NULL COLLATE NOCASE,
  target TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  UNIQUE (profile_id, source)
);
CREATE TRIGGER glossary_terms_follow_profile AFTER DELETE ON profiles
BEGIN
  DELETE FROM glossary_terms WHERE profile_id = OLD.id;
END;
