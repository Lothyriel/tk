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

- **Features**: `.specs/features/voxel-barriers/` and `.specs/features/blood-effects/`.
- **Branch/baseline**: `main`, b845547; clean tree at start. Previous developer-standardization handoff is historical; current Git evidence supersedes its next-step publishing instruction.
- **Authorization**: Implement voxel destructible barriers, then user explicitly requested another agent to add blood/gore. The user subsequently authorized committing both features locally; push/publication is not authorized.
- **Implementation**: Shared 176-cell interior cover; wood 20 HP, concrete 100 HP, server projectile damage and collider removal; protocol 8 full 22-byte destruction bitmask snapshots; client visibility; full raid reset. Arena floor/walls and loot crates remain intact.
- **Voxel verification**: Independent scoped PASS, 4/4 ACs and 2/2 scratch mutants killed; completion validator passed. 44 common + 49 server tests passed independently; 39 client tests passed. One overlapping workspace timing run failed (p95 15.043 ms); unchanged case passed alone. Full gate passed fmt/check then stopped at baseline anatomy.rs:194 Clippy too_many_arguments. See voxel validation.md.
- **Blood effects**: Wound and stump spurts, local/remote/sentry bleeding drips, fresh detached-piece drips, bounded falling voxel droplets and floor stains; cosmetic only. Independent scoped PASS, 4/4 ACs and 3/3 mutants killed after adding an independent world-position assertion. See blood validation.md.
- **Final checks**: Combined serial workspace test run passed 138 Rust tests (45 client, 44 common, 49 server), none ignored; `/tmp/tk-combined-tests-final.log`. Full gate fmt/workspace check passed, but baseline common/src/combat/anatomy.rs:194 Clippy too_many_arguments blocks a full PASS. An initial post-sensor test run reused a mutated scratch binary from the shared Cargo cache; source hashes were unchanged, and a forced real-source rebuild produced the final passing run. Two candidate verification lessons recorded.
- **Delivery**: Voxel barriers committed as c2c54be; blood effects and their validation records are recorded in the following local Conventional Commit. No remaining scoped implementation tasks or authorized publishing actions.
- **Limitations**: No physical rubble/collapse. Blood impacts use the yard floor, without wall decals/fluid simulation. Rendered and real UDP acceptance not executed; headless checks do not certify them. Existing animation artistic acceptance remains open.
