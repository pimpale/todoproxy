CREATE DATABASE todoproxy;
\c todoproxy;

-- Table Structure
-- Primary Key
-- Creation Time
-- Creator User Id (if applicable)
-- Everything else



drop table if exists checkpoint cascade;
create table checkpoint(
  checkpoint_id bigserial primary key,
  creation_time bigint not null default extract(epoch from now()) * 1000,
  creator_user_id bigint not null,
  jsonval text not null
);


drop table if exists operation cascade;
create table operation(
  operation_id bigserial primary key,
  creation_time bigint not null default extract(epoch from now()) * 1000,
  creator_user_id bigint not null,
  jsonval text not null
);









-- drop table if exists checkpoint cascade;
-- create table checkpoint(
--   checkpoint_id bigserial primary key,
--   creation_time bigint not null default extract(epoch from now()) * 1000,
--   creator_user_id bigint not null
-- );
-- 
-- drop table if exists live_task cascade;
-- create table live_task(
--   checkpoint_id bigint not null references checkpoint(checkpoint_id),
--   live_task_id text not null,
--   position i64 not null,
--   value text not null
-- );
-- 
-- drop table if exists finished_task cascade;
-- create table finished_task(
--   checkpoint_id bigint not null references checkpoint(checkpoint_id),
--   finished_task_id text not null,
--   position i64 not null,
--   value text not null,
--   status bigint not null
-- );
