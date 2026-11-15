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

- **Feature**: `.specs/features/local-camera-motion/`, current request to investigate and fix high-FPS movement/camera stutter. The delta-snapshot follow-up was answered as a bandwidth optimization; no protocol expansion was implemented.
- **Branch/baseline**: `main`, `7311e2e`; clean tree at task start. Earlier voxel/blood delivery is committed and historical.
- **Authorization**: User explicitly authorized committing all current camera changes after receiving the existing lint/visual limitations, then requested improved physical destruction. Local commits are authorized; push/publication is not authorized.
- **Implementation**: Replaced local root snapshot assignment with render-frame interpolation on a stable server-time history, using a 100 ms position-display buffer. Mouse look remains immediate. Latest session/gameplay state remains authoritative. First sample, raid/lobby, alive/status/mode transitions, >4 m corrections, >0.25 s receive gaps and timeline restarts reset history. No extrapolation on stale packets. No protocol, server authority or remote interpolation changes.
- **Checks**: Four new headless tests use serialized in-memory Renet messages and real Bevy schedules/transform propagation. Walk/run at 60/165 FPS with 0–8 ms jitter and one lost snapshot have approximately 0.000001 m maximum frame-step error. Literal 3D delayed midpoint, stops/outage bounds, authoritative session data, rotation preservation, discontinuities and old/duplicate sequence rejection pass. Serial workspace tests passed 142 Rust tests (49 client, 44 common, 49 server), zero ignored, `/tmp/tk-camera-workspace-tests.log`.
- **Independent verification**: Scoped behavior PASS for all three criteria; 3/3 isolated behavior mutants killed, 0 survived. Renamed scratch client package isolated its Cargo artifacts; real source hashes and porcelain were unchanged by sensors. See feature `validation.md` for final report and formal verdict.
- **Gate limitation**: `python3 scripts/verify.py --mode full --feature local-camera-motion` passed tooling (20 tests), spec, formatting and workspace check, then failed at existing `common/src/combat/anatomy.rs:194` Clippy argument-count lint (`/tmp/tk-camera-full.log`). Separate client-only Clippy found 13 existing diagnostics outside changed files (`/tmp/tk-camera-client-clippy.log`). No lint was waived; full feature completion remains blocked.
- **Documentation**: Reviewed `docs/GAME_STATE.md` and documented local camera behavior and the 100 ms latency tradeoff; gameplay/Mermaid flow is unchanged.
- **Outstanding acceptance**: Rebuild the client and manually check running past nearby cover and tracking a crate. No rendered client, real UDP harness or video was run. Physical head animation and local movement prediction remain outside scope.
- **Working tree**: Camera changes are prepared for the explicitly requested local commit: client sync module/new motion file, feature spec/report, state handoff and game guide. No unrelated work changed.
