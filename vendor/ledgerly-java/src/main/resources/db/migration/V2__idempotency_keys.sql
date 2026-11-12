create table idempotency_keys (
    user_id         uuid         not null,
    idem_key        varchar(128) not null,
    request_hash    char(64)     not null,
    response_status integer      not null,
    resource_id     uuid         not null,
    created_at      timestamptz  not null,
    expires_at      timestamptz  not null,
    primary key (user_id, idem_key)
);

create index idempotency_keys_expires_idx on idempotency_keys (expires_at);
