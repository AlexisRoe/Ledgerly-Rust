-- Failed logins are now counted by an in-memory Bucket4j limiter (see RateLimiter), not on the user row.
alter table users drop column failed_attempts;
alter table users drop column locked_until;
