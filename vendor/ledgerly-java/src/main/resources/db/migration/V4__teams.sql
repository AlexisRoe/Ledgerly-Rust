create table teams (
    id         uuid         primary key,
    name       varchar(100) not null,
    created_at timestamptz  not null
);

create table memberships (
    team_id   uuid        not null references teams (id),
    user_id   uuid        not null,
    role      varchar(16) not null,
    joined_at timestamptz not null,
    primary key (team_id, user_id)
);

create index memberships_user_idx on memberships (user_id);

create table invitations (
    id         uuid         primary key,
    team_id    uuid         not null references teams (id),
    email      varchar(254) not null,
    role       varchar(16)  not null,
    status     varchar(16)  not null,
    invited_by uuid         not null,
    created_at timestamptz  not null,
    expires_at timestamptz  not null
);

-- Re-inviting an address replaces its pending invitation, so there is at most one per team and address.
create unique index invitations_one_pending_idx on invitations (team_id, email) where status = 'PENDING';
create index invitations_email_idx on invitations (email);
