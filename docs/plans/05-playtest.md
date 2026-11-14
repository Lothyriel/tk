# 05 — Homologação e guia

- [x] Executar check/build/test/fmt/Clippy.
- [x] Iniciar servidor real e cliente renderizado.
- [x] Automatizar lobby → contagem → raid → coleta → extração → debrief → lobby via entrada normal de rede.
- [x] Salvar e revisar screenshots e logs reais.
- [x] Verificação independente da especificação e testes de regressão em cópia isolada.
- [x] Atualizar `docs/GAME_STATE.md` em inglês, incluindo Mermaid e Latest review.

Concluir somente com evidência do ciclo completo. Limitações alfa devem estar explícitas no guia.

Resultado final: 34 testes aprovados; `python scripts/playtest.py --no-build` passou novamente em cliente Vulkan 1280×800 com UDP real. Evidência completa em `target/playtest/`. A segunda raid mantém `secured=1` e reabastece caixas, munição e inimigos. Verifier independente: PASS nos 16 requisitos, 3 mutações detectadas, nenhuma sobrevivente e novo E2E aprovado. Parecer com evidências em `.specs/features/extraction-alpha/validation.md`; `validate_state.py`: 0 erros.
