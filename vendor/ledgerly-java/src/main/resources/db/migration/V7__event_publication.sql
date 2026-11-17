-- Spring Modulith's event publication registry (spring-modulith-events-jdbc 2.1.2, schemas/v2/schema-postgresql.sql).
-- A row is written in the publisher's transaction for every listener of an event and completed when that listener
-- succeeds, so an event whose listener failed or never ran (the application stopped) is still there to be re-delivered.

create table event_publication (
    id                     uuid                     not null,
    listener_id            text                     not null,
    event_type             text                     not null,
    serialized_event       text                     not null,
    publication_date       timestamp with time zone not null,
    completion_date        timestamp with time zone,
    status                 text,
    completion_attempts    int,
    last_resubmission_date timestamp with time zone,
    primary key (id)
);

create index event_publication_serialized_event_hash_idx on event_publication using hash (serialized_event);
create index event_publication_by_completion_date_idx on event_publication (completion_date);
