CREATE DATABASE todoproxy;
\c todoproxy;

-- Table Structure
-- Primary Key
-- Creation Time
-- Creator User Id (if applicable)
-- Everything else

drop table if exists habitica_integration;
create table habitica_integration(
  habitica_integration_id bigserial primary key,
  creation_time bigint not null default extract(epoch from now()) * 1000,
  creator_user_id bigint not null,
  user_id text not null,
  api_key text not null
);

create view recent_habitica_integration_by_user_id as
  select hi.* from habitica_integration hi
  inner join (
    select max(habitica_integration_id) id 
    from habitica_integration
    group by creator_user_id
  ) maxids
  on maxids.id = hi.habitica_integration_id;
