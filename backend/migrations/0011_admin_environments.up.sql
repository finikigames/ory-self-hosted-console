-- Contours an operator attaches to their own admin account (test / prod).
-- The active choice lives on the session so two operators (or two browsers)
-- can point at different stacks without changing the process env defaults.
-- Deleting a contour clears any session still pointing at it.

create table if not exists admin_environments (
    id                uuid        primary key default gen_random_uuid(),
    admin_id          uuid        not null references admins (id) on delete cascade,
    name              text        not null,
    kratos_admin_url  text        not null,
    hydra_admin_url   text        not null,
    hydra_public_url  text        not null,
    keto_read_url     text        null,
    keto_write_url    text        null,
    keto_opl_url      text        null,
    created_at        timestamptz not null default now(),
    updated_at        timestamptz not null default now(),
    constraint admin_environments_name_len check (char_length(name) between 1 and 64),
    constraint admin_environments_unique_name unique (admin_id, name)
);

create index if not exists idx_admin_environments_admin_id
    on admin_environments (admin_id);

alter table sessions
    add column if not exists environment_id uuid null
        references admin_environments (id) on delete set null;
