#!/bin/bash

./target/debug/todoproxy \
  --port=8080 \
  --database-url=postgres://postgres:toor@localhost/todoproxy \
  --auth-service-url=http://localhost:8079 \
  --app-pub-origin=http://localhost:3000 \
  --author-id=3ceedb8e-5ae2-4906-8a7e-749fd597f01d
