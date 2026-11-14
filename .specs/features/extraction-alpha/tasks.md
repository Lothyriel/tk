# Extraction alpha Tasks

Status: Concluído e aprovado pelo Verifier independente. Execução autorizada pelo goal, sem perguntas intermediárias. Sem commits não autorizados.

## Test Coverage Matrix

Diretrizes: `AGENTS.md`: cargo check, cargo test e inicialização dos binários. Não havia testes anteriores.

| Code Layer | Required Test Type | Coverage Expectation | Location Pattern | Run Command |
| --- | --- | --- | --- | --- |
| Domínio | unit | Cada regra e edge case da especificação | common/src/* | cargo test --workspace -j 4 |
| Simulação/rede | integration | Transições, combate real e entrada inválida | server/src/tick/* | cargo test --workspace -j 4 |
| Cliente | unit + e2e | Input/HUD e fluxo renderizado com UDP real | client/src/* | cargo test --workspace -j 4; python scripts/playtest.py |
| Configuração | build | Compilação de ambos os binários | Cargo manifests | cargo build --workspace |

## Gate Check Commands

| Gate Level | When to Use | Command |
| --- | --- | --- |
| Quick | Domínio | cargo test --workspace -j 4 (evita recompilar Bevy com outro conjunto de features do rand) |
| Full | Integração | cargo test --workspace |
| Build | Fechamento | cargo check --workspace; cargo build --workspace; cargo test --workspace; cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings |

## Execution Plan

Uma sequência de seis unidades verificáveis, dentro de um único lote de execução.

```text
T1 -> T2 -> T3 -> T4 -> T5 -> T6
```

## Task Breakdown

### T1: Migrar dependências
Where: workspace
Depends on: None
Done when: Bevy/Renet/Rapier/rkyv atualizados, APIs migradas e build/test passando antes da implementação de gameplay.
Tests: build e testes existentes
Gate: Build
Status: Concluído. Build e testes aprovados; 1 teste de regressão de serialização. Ambos os binários conectados por 8 s sem panic.

### T2: Implementar modelo de combate
Where: common/src/combat.rs
Depends on: T1
Done when: loadouts, três armas, munição, penetração, colete, capacete e corpo têm testes derivados de ALPHA-04 a ALPHA-08.
Tests: unit
Gate: Quick
Status: Concluído. 13 testes passaram (12 de combate + 1 de serialização); cargo check --workspace aprovado.

### T3: Implementar regras da raid
Where: common/src/raid.rs
Depends on: T2
Done when: contagem, início, coleta, extração, morte, timeout e repetição passam nos testes ALPHA-02/03/09/11/12/13.
Tests: unit
Gate: Quick
Status: Concluído. 21 testes passaram (8 de raid, 12 de combate e 1 de serialização); cargo check --workspace aprovado.

### T4: Integrar simulação autoritativa
Where: server/src/tick
Depends on: T3
Done when: mapa comum, IA, física, armas e rede aplicam regras; testes ECS verificam estado final, input inválido e tiro através de cobertura.
Tests: integration
Gate: Full
Status: Concluído. 8 testes ECS/Rapier passaram com 21 testes de domínio; cobrem ciclo completo, morte, timeout, spawn, equipamento trancado, entrada tardia, input inválido/antigo e penetração por espessura real.

### T5: Entregar fluxo visual e controles
Where: client/src
Depends on: T4
Done when: lobby, equipamento, mapa, HUD e resultado consomem snapshots e controles normais permitem jogar do início ao fim.
Tests: unit e inicialização real
Gate: Build
Status: Concluído. 5 testes de input/HUD passaram com 29 testes de domínio/servidor. Build, check, fmt e Clippy com -D warnings aprovados; dois playtests renderizados completos passaram e screenshots foram inspecionados.

### T6: Validar partida completa e documentar
Where: scripts/playtest.py
Depends on: T5
Done when: cliente renderizado completa fluxo real UDP, screenshots são inspecionados, todos os gates passam, docs/GAME_STATE.md e Mermaid correspondem ao jogo e Verifier independente aprova.
Tests: e2e
Gate: Build + python scripts/playtest.py
Status: Concluído. Verifier PASS em validation.md: ALPHA-01..16 comprovados, 34 testes permanentes aprovados, mais 8 provas temporárias isoladas, novo E2E renderizado aprovado, 3 mutações detectadas e nenhuma sobrevivente. Gate validate_state: 0 erros.

## Cross-check

| Task | Depends on | Diagram | Tests co-located |
| --- | --- | --- | --- |
| T1 | None | Root | Build |
| T2 | T1 | T1 -> T2 | Unit |
| T3 | T2 | T2 -> T3 | Unit |
| T4 | T3 | T3 -> T4 | Integration |
| T5 | T4 | T4 -> T5 | Unit/runtime |
| T6 | T5 | T5 -> T6 | E2E |
