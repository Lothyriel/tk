# State

## Decisions

- AD-001: Reutilizar Rapier existente e migrar conjunto compatível Bevy 0.20 / Renet 1.0 / Rapier 0.37.
- AD-002: Jogo super alfa local/LAN, servidor autoritativo, equipamento gratuito e total assegurado apenas em memória.
- AD-003: Executar seis unidades no mesmo goal; não pedir confirmação do próximo plano.
- AD-004: Preservar alterações preexistentes em AGENTS.md, .opencode/ e guia; não commitar sem autorização explícita.
- AD-005: Usuário autorizou criar feature/alfazinho-jogavel e organizar a entrega em commits Conventional Commits, com assuntos e corpos em inglês; push não autorizado.

## Handoff

Feature: extraction-alpha concluída. T1-T6 e ALPHA-01..16 verificados. 34 testes (21 common, 8 server, 5 client), check/build/fmt/Clippy -D warnings aprovados pelo autor e reexecutados pelo Verifier. Playtest renderizado UDP passou até extração e raid 2; cinco PNGs reais inspecionados em target/playtest/. Verifier PASS com 8 provas temporárias adicionais e 3 mutações detectadas, nenhuma sobrevivente; relatório validation.md. Gate validate_state: 0 erros. Guia docs/GAME_STATE.md/Mermaid atualizado com game-state. Sem bloqueios ou tarefas restantes no goal.

Entrega organizada na branch feature/alfazinho-jogavel: d23b9b7 (dependências), 34ee72a (inicialização), d9bb4d8 (combate), 26ecb2d (regras de raid), d756f87 (integração cliente/servidor) e c622ba5 (playtest), seguidos do commit de documentação. Estados intermediários testados a partir de exportações exatas do índice, sem substituir a árvore de trabalho; 1, 13, 21 e 34 testes conforme os módulos disponíveis. E2E do conteúdo preparado passou em target/playtest-commits/. AGENTS.md e .opencode/ preexistentes permanecem fora dos commits. Sem push.
