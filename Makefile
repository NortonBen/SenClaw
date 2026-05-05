HUB_BACKEND_IMAGE ?= senclaw-hub-backend:latest
HUB_BACKEND_TAR ?= senclaw-hub-backend.tar.gz
HUB_BACKEND_COMPOSE ?= docker compose -f hub-backend/docker-compose.yml

.PHONY: hub-build hub-save hub-up hub-down hub-run

hub-build:
	docker build -t $(HUB_BACKEND_IMAGE) ./hub-backend

hub-save:
	docker save $(HUB_BACKEND_IMAGE) | gzip > $(HUB_BACKEND_TAR)

hub-up:
	$(HUB_BACKEND_COMPOSE) up -d --build

hub-down:
	$(HUB_BACKEND_COMPOSE) down

hub-run:
	$(HUB_BACKEND_COMPOSE) run --rm --service-ports hub-backend
