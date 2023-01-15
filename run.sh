#!/bin/bash

./target/debug/todoproxy \
  --port=8080 \
  --database-url=postgres://postgres:toor@localhost/todoproxy \
  --auth-service-url=http://localhost:8079 \
  --app-pub-origin=http://localhost:3000
