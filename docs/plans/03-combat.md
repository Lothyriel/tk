# 03 — Combate super alfa

- [x] Rifle, SMG e pistola, cadência, recarga manual e reserva finita.
- [x] Balística contínua Rapier; madeira penetrável por espessura e concreto impenetrável.
- [x] Colete/capacete com proteção e desgaste; munição AP como escolha.
- [x] Cabeça/tórax/braços/pernas, morte por parte vital e lentidão nas pernas.
- [x] Duas sentinelas hostis para teste solo.
- [x] HUD com corpo, armadura, munição e estado de recarga.

Gate: testes numéricos de dano/penetração e tiros reais sobre colisores.

Evidência: 12 testes de combate em `common/src/combat/tests.rs`; testes Rapier verificam tórax 43 após madeira de 0,24 m e 85 intactos após concreto/madeira espessa. Cliente real eliminou sentinela usando duas balas e chegou à extração com dano no tórax/desgaste do colete, visíveis em `target/playtest/extracting.png`.
