# 02 — Lobby e início

- [x] Mostrar conexão, roster e prontidão.
- [x] Permitir escolher primária, munição, colete e capacete antes de ficar pronto.
- [x] Iniciar com todos prontos (inclusive solo), contagem cancelável de 3 s.
- [x] Trancar equipamento e inicializar raid de 300 s com spawns distintos.
- [x] Deixar entradas tardias aguardando a próxima raid.

Gate: testes de transições, lock de equipamento e inicialização ECS.

Evidência: testes em `common/src/raid/tests.rs` e `server/src/tick/tests.rs`; lobby e segunda partida confirmados pelo cliente real em `target/playtest/client.log`.
