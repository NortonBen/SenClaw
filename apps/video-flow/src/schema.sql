-- Core entity tables (parity with old_backend/schema_core.sql)

CREATE TABLE IF NOT EXISTS character (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    slug                TEXT,
    entity_type         TEXT NOT NULL DEFAULT 'character' CHECK(entity_type IN ('character','location','creature','visual_asset','generic_troop','faction')),
    description         TEXT,
    image_prompt        TEXT,
    voice_description   TEXT,
    reference_image_url TEXT,
    media_id            TEXT,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE TABLE IF NOT EXISTS project (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    description         TEXT,
    story               TEXT,
    story_original      TEXT,
    thumbnail_url       TEXT,
    language            TEXT NOT NULL DEFAULT 'en',
    status              TEXT NOT NULL DEFAULT 'ACTIVE' CHECK(status IN ('ACTIVE','ARCHIVED','DELETED')),
    user_paygate_tier   TEXT NOT NULL DEFAULT 'PAYGATE_TIER_ONE',
    narrator_voice      TEXT,
    narrator_ref_audio  TEXT,
    material            TEXT DEFAULT 'realistic',
    allow_music         INTEGER NOT NULL DEFAULT 0,
    allow_voice         INTEGER NOT NULL DEFAULT 0,
    created_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE TABLE IF NOT EXISTS material (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    style_instruction TEXT NOT NULL,
    negative_prompt   TEXT,
    scene_prefix      TEXT,
    lighting          TEXT DEFAULT 'Studio lighting, highly detailed',
    is_builtin        INTEGER NOT NULL DEFAULT 0,
    created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE TABLE IF NOT EXISTS project_character (
    project_id   TEXT NOT NULL,
    character_id TEXT NOT NULL,
    PRIMARY KEY (project_id, character_id)
);

CREATE TABLE IF NOT EXISTS video (
    id             TEXT PRIMARY KEY,
    project_id     TEXT NOT NULL,
    title          TEXT NOT NULL,
    description    TEXT,
    display_order  INTEGER NOT NULL DEFAULT 0,
    status         TEXT NOT NULL DEFAULT 'DRAFT' CHECK(status IN ('DRAFT','PROCESSING','COMPLETED','FAILED')),
    vertical_url   TEXT,
    horizontal_url TEXT,
    thumbnail_url  TEXT,
    duration       REAL,
    resolution     TEXT,
    orientation    TEXT CHECK(orientation IN ('VERTICAL','HORIZONTAL')),
    youtube_id     TEXT,
    privacy        TEXT NOT NULL DEFAULT 'unlisted',
    tags           TEXT,
    created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE TABLE IF NOT EXISTS scene (
    id              TEXT PRIMARY KEY,
    video_id        TEXT NOT NULL,
    display_order   INTEGER NOT NULL DEFAULT 0,
    prompt          TEXT,
    image_prompt    TEXT,
    video_prompt    TEXT,
    camera_movement TEXT,
    character_names TEXT,
    parent_scene_id TEXT,
    chain_type      TEXT NOT NULL DEFAULT 'ROOT' CHECK(chain_type IN ('ROOT','CONTINUATION','INSERT')),
    source          TEXT NOT NULL DEFAULT 'root' CHECK(source IN ('root','user','system')),

    vertical_image_url        TEXT,
    vertical_image_media_id   TEXT,
    vertical_image_status     TEXT NOT NULL DEFAULT 'PENDING' CHECK(vertical_image_status IN ('PENDING','PROCESSING','COMPLETED','FAILED')),
    vertical_video_url        TEXT,
    vertical_video_media_id   TEXT,
    vertical_video_status     TEXT NOT NULL DEFAULT 'PENDING' CHECK(vertical_video_status IN ('PENDING','PROCESSING','COMPLETED','FAILED')),
    vertical_upscale_url      TEXT,
    vertical_upscale_media_id TEXT,
    vertical_upscale_status   TEXT NOT NULL DEFAULT 'PENDING' CHECK(vertical_upscale_status IN ('PENDING','PROCESSING','COMPLETED','FAILED')),

    horizontal_image_url        TEXT,
    horizontal_image_media_id   TEXT,
    horizontal_image_status     TEXT NOT NULL DEFAULT 'PENDING' CHECK(horizontal_image_status IN ('PENDING','PROCESSING','COMPLETED','FAILED')),
    horizontal_video_url        TEXT,
    horizontal_video_media_id   TEXT,
    horizontal_video_status     TEXT NOT NULL DEFAULT 'PENDING' CHECK(horizontal_video_status IN ('PENDING','PROCESSING','COMPLETED','FAILED')),
    horizontal_upscale_url      TEXT,
    horizontal_upscale_media_id TEXT,
    horizontal_upscale_status   TEXT NOT NULL DEFAULT 'PENDING' CHECK(horizontal_upscale_status IN ('PENDING','PROCESSING','COMPLETED','FAILED')),

    vertical_end_scene_media_id   TEXT,
    horizontal_end_scene_media_id TEXT,

    trim_start        REAL,
    trim_end          REAL,
    duration          REAL,
    transition_prompt TEXT,
    narrator_text     TEXT,
    shot_type         TEXT,

    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_scene_video ON scene(video_id);
CREATE INDEX IF NOT EXISTS idx_scene_order ON scene(video_id, display_order);
CREATE INDEX IF NOT EXISTS idx_video_project ON video(project_id);

CREATE TABLE IF NOT EXISTS pipe_skill_prompt (
    id              TEXT PRIMARY KEY,
    slug            TEXT NOT NULL UNIQUE,
    title           TEXT NOT NULL,
    group_id        TEXT NOT NULL,
    group_title     TEXT NOT NULL,
    display_order   INTEGER NOT NULL DEFAULT 0,
    description     TEXT,
    applies_to      TEXT NOT NULL DEFAULT 'scene_suggest',
    prompt_template TEXT NOT NULL,
    is_active       INTEGER NOT NULL DEFAULT 1,
    version         INTEGER NOT NULL DEFAULT 1,
    created_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at      TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_pipe_skill_prompt_group ON pipe_skill_prompt(group_id, display_order);

CREATE TABLE IF NOT EXISTS project_pipe_skill (
    project_id    TEXT NOT NULL,
    prompt_slug   TEXT NOT NULL,
    enabled       INTEGER NOT NULL DEFAULT 1,
    display_order INTEGER NOT NULL DEFAULT 0,
    updated_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    PRIMARY KEY (project_id, prompt_slug)
);

CREATE INDEX IF NOT EXISTS idx_project_pipe_skill_project ON project_pipe_skill(project_id, enabled, display_order);

CREATE TABLE IF NOT EXISTS app_kv (
    k          TEXT PRIMARY KEY,
    v          TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS project_skill (
    project_id TEXT NOT NULL,
    skill_slug TEXT NOT NULL,
    enabled    INTEGER NOT NULL DEFAULT 1,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (project_id, skill_slug)
);

CREATE TABLE IF NOT EXISTS request (
    id             TEXT PRIMARY KEY,
    project_id     TEXT,
    video_id       TEXT,
    scene_id       TEXT,
    character_id   TEXT,
    type           TEXT NOT NULL CHECK(type IN (
        'GENERATE_IMAGE','REGENERATE_IMAGE','EDIT_IMAGE',
        'GENERATE_VIDEO','REGENERATE_VIDEO','GENERATE_VIDEO_REFS','UPSCALE_VIDEO',
        'GENERATE_CHARACTER_IMAGE','REGENERATE_CHARACTER_IMAGE','EDIT_CHARACTER_IMAGE'
    )),
    orientation    TEXT CHECK(orientation IN ('VERTICAL','HORIZONTAL')),
    status         TEXT NOT NULL DEFAULT 'PENDING' CHECK(status IN ('PENDING','PROCESSING','COMPLETED','FAILED')),
    request_id     TEXT,
    media_id       TEXT,
    output_url     TEXT,
    error_message  TEXT,
    retry_count    INTEGER NOT NULL DEFAULT 0,
    next_retry_at  TEXT,
    edit_prompt    TEXT,
    source_media_id TEXT,
    created_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_request_status ON request(status);
CREATE INDEX IF NOT EXISTS idx_request_scene ON request(scene_id);

CREATE TABLE IF NOT EXISTS skill_agent (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,
    skill_id   TEXT NOT NULL DEFAULT '-',
    skill_ids  TEXT NOT NULL DEFAULT '[]',
    prompt     TEXT NOT NULL DEFAULT '',
    enabled    INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

-- DAG coordination tables (new — multi-agent pipeline)

CREATE TABLE IF NOT EXISTS dag_parents (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'queued' CHECK(status IN ('queued','active','paused','done','failed')),
    goal        TEXT,
    orientation TEXT NOT NULL DEFAULT 'VERTICAL',
    script_md   TEXT,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_dag_parents_project ON dag_parents(project_id);
CREATE INDEX IF NOT EXISTS idx_dag_parents_status ON dag_parents(status);

CREATE TABLE IF NOT EXISTS dag_tasks (
    id              TEXT PRIMARY KEY,
    parent_id       TEXT NOT NULL REFERENCES dag_parents(id),
    label           TEXT NOT NULL,
    agent_type      TEXT NOT NULL,
    prompt          TEXT,
    depends_on      TEXT NOT NULL DEFAULT '[]',
    input_from      TEXT NOT NULL DEFAULT '[]',
    status          TEXT NOT NULL DEFAULT 'registered' CHECK(status IN ('registered','active','done','error','timeout')),
    result          TEXT,
    timeout_seconds INTEGER NOT NULL DEFAULT 900,
    started_at      TEXT,
    completed_at    TEXT
);

CREATE INDEX IF NOT EXISTS idx_dag_tasks_parent ON dag_tasks(parent_id);
CREATE INDEX IF NOT EXISTS idx_dag_tasks_status ON dag_tasks(status);
CREATE UNIQUE INDEX IF NOT EXISTS idx_dag_tasks_label ON dag_tasks(parent_id, label);

CREATE TABLE IF NOT EXISTS media (
    id         TEXT PRIMARY KEY,
    file_name  TEXT NOT NULL,
    file_path  TEXT NOT NULL,
    mime_type  TEXT NOT NULL DEFAULT '',
    size_bytes INTEGER NOT NULL DEFAULT 0,
    media_type TEXT NOT NULL DEFAULT 'other' CHECK(media_type IN ('image','audio','video','other')),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);

CREATE INDEX IF NOT EXISTS idx_media_type ON media(media_type);
