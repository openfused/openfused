# Contributing to OpenFuse

## Getting Started

```bash
git clone https://github.com/velinxs/openfuse.git
cd openfuse
npm install
npm run build
```

## Development

```bash
npm run dev    # watch mode
```

Test locally:
```bash
node dist/cli.js init --name test-agent
node dist/cli.js status
node dist/cli.js inbox send peer-1 "hello"
node dist/cli.js inbox list
```

## Project Structure

```
src/
  cli.ts       — CLI entry point (commander)
  store.ts     — Context store CRUD operations
  watch.ts     — File watchers (chokidar) for inbox + context changes
templates/
  CONTEXT.md   — Default context template
  SOUL.md      — Default soul template
```

## Guidelines

- Keep it simple. Files are the protocol.
- No unnecessary dependencies.
- TypeScript strict mode.
- If an agent can't understand it by reading files, it's too complex.
