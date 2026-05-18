# Build stage: install deps
FROM oven/bun:1 AS build
WORKDIR /app
COPY package.json bun.lock* ./
COPY packages/chat-client/package.json ./packages/chat-client/
COPY web/package.json ./web/
RUN bun install --frozen-lockfile

# Runtime
FROM oven/bun:1-slim
WORKDIR /app
COPY --from=build /app/node_modules ./node_modules
COPY package.json tsconfig.json drizzle.config.ts ./
COPY src ./src
COPY packages ./packages
COPY drizzle ./drizzle

ENV NEXAL_HTTP_PORT=3000
EXPOSE 3000

ENTRYPOINT ["bun", "run", "src/cli.ts"]
