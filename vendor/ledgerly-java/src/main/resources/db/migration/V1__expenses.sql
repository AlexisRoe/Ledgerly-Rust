create table expenses (
    id           uuid          primary key,
    team_id      uuid          not null,
    claimant_id  uuid          not null,
    amount       numeric(19,4) not null,
    currency     char(3)       not null,
    category     varchar(32)   not null,
    incurred_on  date          not null,
    merchant     varchar(200)  not null,
    note         varchar(2000),
    status       varchar(32)   not null,
    version      bigint        not null,
    created_at   timestamptz   not null,
    updated_at   timestamptz   not null
);

create index expenses_team_incurred_idx on expenses (team_id, incurred_on desc, id);
create index expenses_team_claimant_idx on expenses (team_id, claimant_id);
create index expenses_team_status_idx on expenses (team_id, status);

create table expense_rejections (
    expense_id  uuid          not null references expenses (id) on delete cascade,
    position    integer       not null,
    rejected_by uuid          not null,
    reason      varchar(1000) not null,
    primary key (expense_id, position)
);
