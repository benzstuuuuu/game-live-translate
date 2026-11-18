CREATE TABLE profiles (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  source TEXT NOT NULL DEFAULT 'subtitle' CHECK (source IN ('subtitle', 'audio')),
  region_x REAL, region_y REAL, region_w REAL, region_h REAL,
  bar_x REAL, bar_y REAL, bar_w REAL,
  created_at INTEGER NOT NULL,
  last_used_at INTEGER NOT NULL
);
