ALTER TABLE community ADD COLUMN followers_url TEXT NOT NULL DEFAULT generate_unique_changeme();
ALTER TABLE community ADD COLUMN inbox_url TEXT NOT NULL DEFAULT generate_unique_changeme();
ALTER TABLE community ADD COLUMN shared_inbox_url TEXT;

ALTER TABLE user_ ADD COLUMN inbox_url TEXT NOT NULL DEFAULT generate_unique_changeme();
ALTER TABLE user_ ADD COLUMN shared_inbox_url TEXT;

ALTER TABLE community ADD CONSTRAINT idx_community_followers_url UNIQUE (followers_url);
ALTER TABLE community ADD CONSTRAINT idx_community_inbox_url UNIQUE (inbox_url);
ALTER TABLE user_ ADD CONSTRAINT idx_user_inbox_url UNIQUE (inbox_url);
