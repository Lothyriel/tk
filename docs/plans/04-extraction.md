# 04 — Coleta, extração e repetição

- [x] Três caixas laranja, F para coletar perto, coleta única no servidor.
- [x] Zona verde: segurar F por 5 s carregando saque.
- [x] Cancelar progresso ao sair/soltar; creditar saque exatamente uma vez.
- [x] Morte e timeout perdem saque, sem respawn na mesma raid.
- [x] Debrief quando todos terminarem; Enter devolve ao lobby.
- [x] Segunda raid limpa inimigos/projéteis/caixas sem perder total assegurado.

Gate: testes de sucesso, cancelamento, morte, timeout e segunda partida.

Evidência: regras e integração ECS aprovadas; `target/playtest/server.log` registra coleta, extração com `secured=1`, debrief, lobby e início da raid 2. O teste renderizado também afirma caixas reabastecidas, munição completa e duas sentinelas novas na segunda raid.
