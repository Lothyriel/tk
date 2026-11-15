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

## Handoff

### Active goal and publishing policy

Execute `C:/Users/sandr/Documents/ChatGPT/TK/plano-tk-animacoes-ragdoll.md` on `feature/animacoes-mitologicas`. The earlier physical-humanoids delivery at705d035 is accepted; this animation/ADS/ragdoll goal is not. Models were accepted as a first delivery, animation quality was not. Blood/liquids remain excluded. Preserve preexisting AGENTS.md/.opencode and the manual old server on UDP9080; harnesses own19080/19081 only.

Commit and push every coherent verified unit immediately (AD-012/013), with English Conventional Commits, explicit paths, normal hooks and no force-push. Published so far:

- `5dbad93 feat(rig)!: add weapon choreography and replicated physical poses` — exact indexed snapshot fmt/check/42common tests PASS.
- `048c6f1 feat(gameplay): integrate aimed weapons and articulated recovery` — exact indexed snapshot fmt/check/full workspace tests PASS.
- `d006968 fix(combat): resolve point-blank clearance against posed anatomy` — exact indexed snapshot fmt/check/full workspace tests PASS (43server tests).
- `154efc9 test(playtest): capture native animation and physical comparisons` — exact indexed snapshot fmt/check/111workspace tests/Python syntax and both negative provenance guards PASS.
- `c0c6af3 docs(game): document animation checkpoints and open acceptance gates` — guide/spec/state and preliminary static review published.

All five commits above were pushed after their gates. Candidate16 has completed. The next isolated unit is a client-only M1911 receiver-pivot/final-camera framing correction with real-mesh tests and its guide/spec/evidence updates. Its exact indexed fmt/check/112tests passed (27client/42common/43server), and disabling the framing system in an isolated scratch correctly failed on actual mesh clipping. The first gate attempt was interrupted by a harness shutdown; attempt2 completed, with no real source mutated. No owned build/native/performance jobs are active. Commit/push the verified framing unit immediately, then freeze17; native/artistic acceptance is still open. Exclude preexisting AGENTS.md/.opencode. Logs are in `target/playtest/commit-gates/06-pistol-framing/`.

### Current implementation

Protocol5: held-RMB ADS, actual shared iron-sight/barrel anchors,70degree world/65degree fixed weapon lens,52pistol/44primary eased zoom, input gating, human cadence and restrained bob/sway. Original M1911/MP5/M4A4 assemblies share five weapon parts and action contacts in both views. Empty/tactical handling differs; MP5 handle does not reciprocate on fire and its interrupted lock settles continuously. The overhand wrist and2/3camera-facing rail guard improved slide readability in16, but its actual empty-reload shot cropped the pistol at the right edge. The new client-only correction pivots cant about the receiver instead of the eye, fits cached real-mesh envelopes to the final camera, preserves fixed lens/centred ADS and eases fitting out during deliberate lowering. Camera-relative sleeves are rebuilt from corrected cuffs to torso anchors. Five-aspect120Hz actual-vertex/empty/tactical/ADS/fall tests pass alongside the unchanged cuff/elbow/rail assertions; fresh17 native review is still required. Shared choreography, barrel origins and server rules are unchanged.

Server-owned planted contacts, same-ID articulated surviving fall/get-up/death/separation and compact actual poses are implemented. Loose fragments are excluded from KCC support. Six additional solver iterations, original damping and original load/rest bounds remain. Maximum snapshot59,784bytes leaves216bytes below the unchanged60,000 limit; do not casually add replicated fields.

User-approved point-blank repair uses server-only MuzzleClearance: resolve eye-to-muzzle through the same posed-anatomy/world/penetration loop as flight, then retain the original muzzle ray if clear. No protocol growth or ammo/damage/cadence tuning. Six new integration tests cover24weapon/aim/distance head cases with exact damage/ammo/muzzle, real pre-muzzle bodies, narrow concrete, posed gaps, and wood charged once even when the muzzle starts inside it. Diagnostics are opt-in and record exact origins/contacts. Current suite count is112 (27client+42common+43server).

### Evidence and remaining acceptance

- Candidate14: fmt/check/110tests and continuous dual UDP/loss PASS, including original fatal-shot flow and matching15-piece recovery IDs. Videos3243/3417real samples,128.60/128.56s, full decode PASS. Native views still exposed M1911 sleeve occlusion; the newest wrist/visibility fix postdates it.
- Candidate15:111tests PASS; native FAIL on old16.8s recovery assertion. Server safely began get-up at16.04s and finishedStanding17.04s. AD-011 now captures actual completion with18.2s bound, preserving physical gates,1sget-up and fatal deadline. Do not relabel15PASS or silently retry/force unsafe recovery. Its planned acceptance/performance pipeline never ran.
- Candidate16: root fmt/check/111tests and full diagnostic dual UDP/loss PASS, including actual recovery bounded18.2s and the original fatal-shot assertion. Videos3098/3045real samples,129.64s each, full decode PASS; milestones1280x800. Adjacent slide/get-up boards were inspected. The M1911 right-edge crop is a retained visual failure, not artistic acceptance; it motivates the current framing correction.
- Earlier protocol4 candidate10 dual and healthy4-case matrix passed, but its animation/FOV quality was user-rejected. Historical before/candidate videos and failures remain under ignored target/playtest, indexed in docs/ANIMATION_RAGDOLL.md. Public Khoa/Tactile clips were inspected as extracted real reference boards, not claimed normal-speed viewing or imported art.
- `animations-matched-before-14` is prepared and built from705d035 with only recorder/ordinary driver/package aliases plus explicitly fingerprinted70/65camera matching. The exact historical Python runner is retained/hash-checked; baseline wrapper rejects candidates. Actual native settings must match, including diagnostic/video flags and viewport. Its new native/video/performance runs are still pending.
- Next freeze: `animations-ads-candidate-17`, after the framing commit/push. Run diagnostic continuous dual/video, inspect new slide/ADS/get-up frames, then healthy matrix, solo, no-video dual and matched before/after videos/metrics/load. The sequential artifact runner is `target/playtest/animations-ads-candidate-13/run_acceptance.py AFTER BEFORE`; use before14 export. Original baseline physical failures must remainFAIL while retaining eight decoded clips. Do not run competing workloads or prompt video playback during captures/benchmarks. Strict screenshot dimensions must stay enabled (an older minimized1x1 run correctly failed).
- Fresh independent Verifier session `ses_e234d8142ffe1Y46Bsn3EjZo2m` completed static-only `validation-static.md`; it has run no cargo/native/sensor. Resume after final evidence, provide new AD-011/wrist changes, and require isolated-scratch mutation sensor plus truthful final report. No final validation.md/PASS exists. Its quantitative interpolation/contact/force coverage caveats and final art UAT remain open.

This is a published implementation checkpoint, not completion or near-AAA certification. Keep tasks/spec/guide/Mermaid honest, finish technical and artistic acceptance, and keep committing/pushing each verified follow-up. The verbose chronological handoff was preserved at ignored `target/playtest/commit-gates/STATE-before-checkpoint-commits-2026-11-14.md` rather than discarded.
