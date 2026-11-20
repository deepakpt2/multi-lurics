# syntax=docker/dockerfile:1.7
# Next.js standalone server. API_ORIGIN is baked in at build time (Next rewrites).

FROM node:20-alpine AS deps
WORKDIR /app
COPY package.json package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci --no-audit --no-fund

FROM node:20-alpine AS build
WORKDIR /app
ARG API_ORIGIN=http://api:8787
ENV API_ORIGIN=${API_ORIGIN} NEXT_OUTPUT_STANDALONE=1 NEXT_TELEMETRY_DISABLED=1
COPY --from=deps /app/node_modules ./node_modules
COPY . .
RUN mkdir -p public && npm run build

FROM node:20-alpine
WORKDIR /app
# The server renders public pages by calling the API itself, so it needs the origin at run time too.
ARG API_ORIGIN=http://api:8787
ENV API_ORIGIN=${API_ORIGIN}
ENV NODE_ENV=production NEXT_TELEMETRY_DISABLED=1 PORT=3000 HOSTNAME=0.0.0.0
COPY --from=build --chown=node:node /app/.next/standalone ./
COPY --from=build --chown=node:node /app/.next/static ./.next/static
COPY --from=build --chown=node:node /app/public ./public
USER node
EXPOSE 3000
CMD ["node", "server.js"]
