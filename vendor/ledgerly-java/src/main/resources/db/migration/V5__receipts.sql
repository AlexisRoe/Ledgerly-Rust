create table receipts (
    id                 uuid         primary key,
    team_id            uuid         not null,
    expense_id         uuid         not null,
    storage_key        varchar(200) not null,
    original_name      varchar(255) not null,
    detected_type      varchar(32)  not null,
    size_bytes         bigint       not null,
    sha256             varchar(64)  not null,
    scan_status        varchar(16)  not null,
    possible_duplicate boolean      not null,
    uploaded_by        uuid         not null,
    created_at         timestamptz  not null
);

-- The same file twice on one expense is rejected by the database itself, so two simultaneous uploads cannot both win.
create unique index receipts_expense_sha_uk on receipts (expense_id, sha256);
-- Finds the same file on other expenses of a team (possible duplicate).
create index receipts_team_sha_idx on receipts (team_id, sha256);
create index receipts_team_expense_idx on receipts (team_id, expense_id);
