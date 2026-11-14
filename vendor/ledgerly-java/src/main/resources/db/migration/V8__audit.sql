-- The audit trail: who did what, when. Written from domain events, never edited, never deleted.
create table audit_events (
    id           uuid         primary key,
    -- The id of the domain event this row records. Unique, so delivering an event twice records it once.
    event_id     uuid         not null unique,
    -- Null for events that belong to no team (registering, verifying): they never show up in a team's trail.
    team_id      uuid,
    -- Null when the system acted (a malware scan result).
    actor_id     uuid,
    event_type   varchar(60)  not null,
    subject_type varchar(30),
    subject_id   uuid,
    details      jsonb        not null default '{}'::jsonb,
    occurred_at  timestamptz  not null,
    -- When the row was written. The trail is paged by (created_at, id): rows are only ever added at the end, so
    -- a client paging with a cursor never misses one that arrives late.
    created_at   timestamptz  not null
);

create index audit_events_team_idx on audit_events (team_id, created_at desc, id desc);

-- Append-only, on two levels. First, nobody may update, delete or truncate: the privileges are taken away from
-- everyone, including the role that owns the table and runs the application.
revoke update, delete, truncate on audit_events from public;
do $$
begin
    execute format('revoke update, delete, truncate on audit_events from %I', current_user);
end
$$;

-- Second, a trigger refuses the same statements. A superuser ignores privileges (the throw-away database in the
-- tests runs as one), but not triggers, and this keeps the rule true wherever the schema is installed.
create function audit_events_refuse_changes() returns trigger
    language plpgsql as
$$
begin
    raise exception 'audit_events is append-only: % is not allowed', tg_op;
end
$$;

create trigger audit_events_no_update_delete
    before update or delete on audit_events
    for each row execute function audit_events_refuse_changes();

create trigger audit_events_no_truncate
    before truncate on audit_events
    for each statement execute function audit_events_refuse_changes();
