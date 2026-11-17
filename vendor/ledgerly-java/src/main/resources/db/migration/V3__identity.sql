create table users (
    id              uuid         primary key,
    email           varchar(254) not null unique,
    password_hash   varchar(255) not null,
    verified        boolean      not null,
    failed_attempts integer      not null,
    locked_until    timestamptz,
    created_at      timestamptz  not null
);

create table email_verifications (
    id         uuid        primary key,
    user_id    uuid        not null references users (id),
    token_hash varchar(64) not null unique,
    expires_at timestamptz not null,
    used_at    timestamptz
);

create table refresh_tokens (
    id          uuid        primary key,
    family_id   uuid        not null,
    user_id     uuid        not null references users (id),
    token_hash  varchar(64) not null unique,
    created_at  timestamptz not null,
    expires_at  timestamptz not null,
    revoked_at  timestamptz,
    replaced_by uuid
);

create index refresh_tokens_family_idx on refresh_tokens (family_id);
