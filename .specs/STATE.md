# State

## Decisions

- AD-001: Reutilizar Rapier existente e migrar conjunto compatível Bevy 0.20 / Renet 1.0 / Rapier 0.37.
- AD-002: Jogo super alfa local/LAN, servidor autoritativo, equipamento gratuito e total assegurado apenas em memória.
- AD-003: Executar seis unidades no mesmo goal; não pedir confirmação do próximo plano.
- AD-004: Preservar alterações preexistentes em AGENTS.md, .opencode/ e guia; não commitar sem autorização explícita.
- AD-005: Usuário autorizou criar feature/alfazinho-jogavel e organizar a entrega em commits Conventional Commits, com assuntos e corpos em inglês; push não autorizado.
- AD-006: Após o aceite de physical-humanoids, usuário autorizou separar a entrega em Conventional Commits e fazer push na nova branch feature/animacoes-mitologicas; não autorizou merge, deploy ou inclusão de alterações concorrentes/preexistentes.
- AD-007: Usuário aprovou corrigir somente a mira da fixture next_deploy_restores_separated_anatomy_and_clears_fragments_corpses_and_effects para a cabeça real do rig após a queda; preservar todas as asserções de morte/debrief/reset/contagem e o filtro correto do controlador.
- AD-008: Na revisão dos vídeos da candidata10, usuário aceitou os modelos como primeira entrega, mas rejeitou o acabamento das animações (movimentação da arma muito rápida/não natural, reload sem manipulação clara, FOV estranho) e pediu ADS no botão direito com animações muito refinadas; manter o aceite artístico aberto e executar esse refinamento no mesmo goal.
- AD-009: Usuário aprovou substituir o requisito arbitrário de distância de40cm da nova asserção ADS pelo alinhamento da mira à frente do near plane de0.1m; não alterar testes antigos nem critérios de munição/dano/rede.
- AD-010: Após a falha nativa13, usuário escolheu explicitamente "Corrigir combate também": autorizou investigar e corrigir a origem do tiro à queima-roupa, com regressões para jogadores/hip/ADS/cobertura. Não alterar a rota para esconder a falha; preservar asserções e prazos, dano/munição/cadência e evidência reprovada13.
- AD-011: Usuário aprovou registrar a conclusão real da recuperação com limite18.2s após a queda física da candidata15 terminarStanding17.04s (checkpoint antigo16.8s falhou0.23s antes). Preservar a asserçãoStanding, IDs, get-up1s, critérios deapoio/quietude/espaço e prazo do tiro fatal; não forçar recuperação precoce. Falha15 permaneceFAIL.
- AD-012: Usuário autorizou commitar a entrega em Conventional Commits em inglês e fazer push para a branch remota feature/animacoes-mitologicas. Excluir AGENTS.md/.opencode/ preexistentes e artefatosignored, não fazer merge/deploy/release nem afirmar aceite artístico ainda pendente.
- AD-013: Usuário determinou commitar e pushar continuamente a partir de agora, sem acumular muitas alterações locais. Cada unidade coerente e validada deve ser publicada imediatamente na mesma branch autorizada; preservar trabalho preexistente, gates e hooks, sem force-push/merge/deploy.

## Scope of historical records

AD-001 through AD-013 above are preserved verbatim. Feature/session-specific instructions and publishing permissions (especially AD-003 through AD-013) apply only to their original context; they do not grant standing authority to future tasks. The current request, root guide and reconciled Git/task evidence govern new work.

The complete previous state, including its handoff, personal paths, session IDs and claimed running pipeline, is archived verbatim at [history/animation-ragdoll-state-2026-11-14.md](history/animation-ragdoll-state-2026-11-14.md), from baseline 83bfc4a. Those runtime claims have not been re-established in this worktree and are not active instructions. Do not resume recordings or publish on their authority.

## Handoff

- **Feature**: `.specs/features/developer-standardization/`.
- **Branch**: `chore/headless-validation-instructions`, created from synchronized main at 311df3a; initial headless/.opencode work is published as 0912c9a and 83bfc4a.
- **Phase / Task**: T1-T4 implementation complete; independent final verification is still pending.
- **Completed**: T1 local TLC 3.3.0 snapshot with provenance/licenses (c0e1ff5), T2 portable verification CLI with tests (2184e34), T3 short root guide, focused skill routing and contributor guide (e6c3fc8), T4 reconciled memory/history and unchanged-gameplay guide review. Three routing simulations, 20 tooling tests, both structural validators and direct baseline comparisons passed. Separately executed workspace tests passed 112 tests (27 client, 42 common, 43 server), without a client/video.
- **In-progress**: Independent final review and scratch-only discrimination sensor, then completion report/publication. The real full run passed tooling/structural checks, Rust formatting and workspace check, then stopped with exit 101 at the preexisting common/src/combat/anatomy.rs:194 Clippy too_many_arguments lint; its workspace-test step was not reached. Logs: ignored target/verification/developer-standardization-{full,workspace-tests}.log (separate runs).
- **Next step**: Commit T4, obtain independent verification and pass the formal completion gate for the scoped tooling delivery, then publish to the same authorized branch. No PR, merge, release or deploy is authorized.
- **Blockers / exclusions**: Full remains blocked by the existing Clippy lint; the user accepted that limitation for this standardization delivery only. Do not suppress the warning or report full as passed. CI, toolchain pinning, Rust editions, linker settings and gameplay remain out of scope. Default verification is headless; video is opt-in.
- **Uncommitted work**: Only this task's memory/history and guide changes in this worktree; re-check Git rather than treating this snapshot as live status. The original C:/repos/tk worktree has preexisting AGENTS.md/test/.opencode changes that remain outside this delivery.
- **Prior gameplay acceptance**: Animation/ragdoll implementation was merged into main via 311df3a, but the tracked tasks and static reports still retain open visual/performance/final-acceptance criteria. No final animation-ragdoll validation.md/PASS is recorded. This tooling task does not resolve or rerun those criteria. See the feature records and docs/ANIMATION_RAGDOLL.md; do not infer acceptance from the merge or stale process IDs.
