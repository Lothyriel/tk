# Validation: extraction-alpha - PASS

**Verdict:** PASS

O fluxo especificado passou na verificação independente: 34 testes entregues, oito provas adicionais somente na cópia isolada, cliente renderizado com servidor UDP real e três mutações comportamentais detectadas. Nenhuma falha funcional reproduzível foi encontrada no escopo ALPHA-01..16.

- **Data:** 2026-11-14.
- **Verifier:** subagente independente, não autor da implementação.
- **Fonte de verdade:** `.specs/features/extraction-alpha/spec.md`, acompanhada de `design.md` e `tasks.md`; procedimento integral de `tlc-spec-driven/references/validate.md`.
- **Diff range:** `HEAD (4becb458e0c17fe34f8cab793018ba5cbb8b883f) + working tree`, incluindo arquivos novos de `common/`, `client/`, `server/`, `scripts/`, `docs/` e `.specs/`, além dos manifests, lockfile e configuração de build alterados. Não é uma comparação limitada a arquivos rastreados.
- **Preservação:** `AGENTS.md` e `.opencode/` são alterações preexistentes e não foram editados. Nenhum código/teste real foi modificado. Nenhum commit, push ou stash foi criado. O único arquivo escrito na árvore fonte por este Verifier é este relatório.

## Localização das evidências

Os caminhos sem prefixo são relativos a `C:/repos/tk`. Os prefixos abaixo identificam arquivos externos reais, não arquivos entregues:

- **`SCRATCH/`** = `C:/Users/sandr/AppData/Local/Temp/opencode/extraction-verifier-15bb51bfa73049e882bf6fffcded21f6/`.
- **`E2E/`** = `C:/Users/sandr/AppData/Local/Temp/opencode/extraction-verifier-e2e-20261114/`.

O scratch foi criado copiando somente `Cargo.toml`, `Cargo.lock`, `.cargo/`, `common/`, `client/` e `server/`. Logs e provas independentes foram adicionados posteriormente apenas ali. Foi preservado para auditoria. As oito provas suplementares não fazem parte dos 34 testes permanentes e não foram incorporadas ao projeto. Seus resultados demonstram os comportamentos nesta revisão, não prometem cobertura permanente futura.

## Cobertura independente por requisito

Valores esperados foram derivados da especificação, não dos comentários ou resultados declarados pelo autor. As assertions abaixo foram executadas e passaram sobre código sem mutações, exceto as falhas intencionais registradas no sensor.

| Requisito / condição | Resultado exigido e valor esperado | Evidência exata + assertion | Parecer |
| --- | --- | --- | --- |
| ALPHA-01: dependências | Bevy 0.20.1; Renet 1.0.0; Rapier 0.37.0; rkyv 0.8.18; workspace compila | `Cargo.toml:19` a `Cargo.toml:22`; `Cargo.lock:447`, `Cargo.lock:1454`, `Cargo.lock:1581`, `Cargo.lock:5216`. Prova Python/tomllib executada: `assert actual == {k: [v] for k, v in expected.items()}`, com as quatro versões exatas; `cargo check` e `cargo build` retornaram 0. | PASS |
| ALPHA-02: lobby, equipamento e roster | Lobby visível, escolhas e pronto/não pronto por jogador | `client/src/hud.rs:171` / `:173`: `assert!(text.contains("LOBBY"))`, `assert!(text.contains("[G] Primary"))`; `client/src/input/mod.rs:194` a `:199`: primária SMG, AP, Heavy, capacete desligado e sequência ready 1; `SCRATCH/client/src/hud.rs:191` / `:192`: `text.contains("YOU 0007   READY")` e `text.contains("PMC 0008   NOT READY")`. `E2E/lobby.png` inspecionado. | PASS |
| ALPHA-02: lobby não permite combater | Posição inalterada e zero projéteis; input não aciona movimento/tiro/interação | `server/src/tick/tests.rs:76`: `assert_eq!(translation, before)`; `:77` a `:83`: contagem de `Projectile == 0`; `client/src/input/mod.rs:200` a `:202`: `!input.forward`, `!input.fire`, `!input.interact`. | PASS |
| ALPHA-03: todos prontos, inclusive solo | Countdown de 3 s, depois Active com 300 s | `common/src/raid/tests.rs:8` a `:15`: `phase == Countdown`, `seconds_left == 3.0`, depois `1.0`, `phase == Active`, `seconds_left == 300.0`, `raid_id == 1`; `server/src/tick/tests.rs:86` / `:89`: permanece Lobby com apenas um de dois prontos e inicia Active quando ambos prontos. `E2E/server.log:3` / `:4`: intervalo real de aproximadamente 3 s. | PASS |
| ALPHA-03: inicialização | Spawns distintos e equipamento cheio | `server/src/tick/tests.rs:92`: `delta.length() >= 1.4`; `client/src/playtest.rs:123` a `:128`: loadout desejado, colete `100.0`, capacete `40.0`, corpo `[35.0,85.0,60.0,70.0]`, carregador `30` e dois bots; `SCRATCH/common/src/combat/tests.rs:6` / `:7`: carregadores `[30,17,25]`, reservas `[90,51,75]`. | PASS |
| ALPHA-03: cancelar contagem | Lobby, sem início de raid | `common/src/raid/tests.rs:27` / `:28`: `phase == Lobby`, `seconds_left == 0.0`; `SCRATCH/server/src/tick/tests.rs:60` a `:62`: Lobby, participante não pronto e bots `0` após cancelar via sequência normal. | PASS |
| ALPHA-03: entrada tardia | Waiting, sem combate até próximo lobby | `server/src/tick/tests.rs:130` a `:134`: `status == RaidStatus::Waiting`, presença de `ColliderDisabled`; bloqueios autoritativos em `server/src/tick/simulation.rs:39` e `:122` exigem status Active. | PASS |
| ALPHA-04: trava de equipamento | Alterações rejeitadas enquanto pronto e na raid | `SCRATCH/server/src/tick/tests.rs:54` / `:57`: Countdown e loadout continua `chosen` após input diferente; `server/src/tick/tests.rs:119` a `:126`: loadout permanece o selecionado e arma continua SMG apesar da tentativa de Rifle. | PASS |
| ALPHA-04: armas autorizadas | Apenas primária selecionada + pistola | `common/src/combat/tests.rs:10` a `:12`: `allows(Smg)`, `allows(Pistol)`, `!allows(Rifle)`; `:209` a `:211`: tentativa de selecionar Rifle não muda a pistola ativa. | PASS |
| ALPHA-05: armas e modos | Rifle 30 automático; SMG 25 automática; pistola 17 semiautomática | `common/src/combat/tests.rs:26` a `:31`: tamanhos `30/25/17` e flags `true/true/false`; `:149` a `:161`: Rifle dispara, respeita cadência, volta a disparar segurando; pistola só volta a disparar com nova sequência, carregador `16` e depois `15`. | PASS |
| ALPHA-05: reload e reserva finita | Recarga manual temporizada; três carregadores em reserva por arma; não cria munição | `common/src/combat/tests.rs:173` a `:186`: sem comando carregador fica `0`; comando inicia `2.4` s, após 1 s ainda `0`, ao concluir recebe apenas `2`, reserva `0`, nova recarga não inicia; `SCRATCH/common/src/combat/tests.rs:7`: reservas `[90,51,75] == 3 * [30,17,25]`. | PASS |
| ALPHA-06: madeira e concreto | Madeira custa 20 unidades/m, só atravessa com energia >0; concreto sempre para | `common/src/combat/tests.rs:44` a `:47`: `penetrate(30,0.5) == Some(20)`, `penetrate(10,0.5) == None`, madeira 2 m para, concreto para energia `1000`; `server/src/tick/tests.rs:308` a `:312`: saúde torácica `43 ±0.1` após madeira real 0.24 m e zero projéteis; `:317` / `:318`: concreto e madeira espessa resultam `(85.0,0)`. | PASS |
| ALPHA-07: colete, blunt e desgaste | Bloqueado aplica 20% de dano; desgaste 40% do dano bruto | `common/src/combat/tests.rs:59` a `:61`: acerto bruto 50 bloqueado deixa tórax `75` (85−10) e colete `80` (100−20). | PASS |
| ALPHA-07: penetração e capacete | Penetrante aplica 80%; capacete cobre cabeça, não pernas | `common/src/combat/tests.rs:72` a `:77`: bruto 50 penetrante deixa tórax `45` (85−40), colete `80`; braço recebe dano integral; `:87` a `:95`: cabeça `31` após bruto20 bloqueado, capacete `32`, pernas `60`, capacete quebrado não protege. | PASS |
| ALPHA-08: corpo e morte | Cabeça35, tórax85, braços60, pernas70; vital zero mata | `common/src/combat/tests.rs:102`: `body.parts == [35.0,85.0,60.0,70.0]`; `:106` / `:110`: `!head.alive()`, `!thorax.alive()` após zerar cada vital. | PASS |
| ALPHA-08: pernas | Abaixo de35 velocidade 55%; em35 sem penalidade | `common/src/combat/tests.rs:117`: `speed_factor() == 0.55`; `SCRATCH/common/src/combat/tests.rs:10` / `:12`: em35 fator `1.0`, em34.99 fator `0.55`. Integração aplica esse fator em `server/src/tick/simulation.rs:79`. | PASS |
| ALPHA-09: coletar e consumir | F, participante ativo e disponível, distância ≤2 m; crédito1 e indisponibilidade global | `common/src/raid/tests.rs:45` a `:54`: longe/sem F rejeitados, `carried == 1`, `!loot_available[0]`, repetição e segundo jogador rejeitados; `SCRATCH/common/src/raid/tests.rs:9` / `:10`: 2.01 m rejeita, 2.0 m aceita; `server/src/tick/tests.rs:152`: carried1 na simulação. | PASS |
| ALPHA-10: combate solo | Duas sentinelas hostis disparam em participante vivo visível | `server/src/tick/tests.rs:93`: bots `==2`; `SCRATCH/server/src/tick/tests.rs:76` / `:77`: `ammo < 17` na pistola da sentinela e `body.total() < 250.0` no jogador visível; `:80`: munição não diminui com concreto obstruindo visão. `client/src/playtest.rs:146` / `:147` e `E2E/client.log:15`: kill autoritativo observado com munição28. | PASS |
| ALPHA-11: tempo e crédito | 5 s contínuos; Extracted; crédito de todo saque exatamente uma vez | `common/src/raid/tests.rs:65` a `:73`: em4 s não extrai, progresso4 e secured0; +1 s extrai, status Extracted, secured2, carried0; repetição mantém secured2. E2E valida secured1/carried0 em `client/src/playtest.rs:189` / `:190`. | PASS |
| ALPHA-11: limites da zona | Horizontal ≤2.5 m e vertical ≤2 m | `SCRATCH/common/src/raid/tests.rs:12` a `:20`: horizontal2.51 e vertical2.01 rejeitam e secured0; horizontal2.5 e vertical2.0 aceitam, secured1 e depois2. | PASS |
| ALPHA-11: interrupção | Soltar F ou sair da zona zera progresso | `common/src/raid/tests.rs:85`, `:88`, `:91`, `:92`: progresso `0.0` após soltar, sair horizontalmente ou verticalmente; secured `0`. | PASS |
| ALPHA-11: não elegíveis | Sem saque, morto, Waiting ou fora de Active não recebe crédito | `common/src/raid/tests.rs:103`, `:106`, `:109`, `:114`, `:116`: todas as tentativas retornam `false` e secured permanece `0`. | PASS |
| ALPHA-12: morte e timeout | Perde carried; Dead ou Missing; sem respawn | `server/src/tick/tests.rs:202` a `:207`: Dead, carried0 e cabeça0 mesmo com jump; `:218` a `:223`: Missing, carried0 e secured0; `common/src/raid/tests.rs:123` a `:125`: timeout somente ao completar300 s. | PASS |
| ALPHA-12: ações posteriores | Dead/Missing não movem, atiram nem extraem | `SCRATCH/server/src/tick/tests.rs:101` a `:106`: para ambos os estados, posição igual à anterior, carregador continua30, status preservado, secured0 e progresso0 mesmo com input renovado forward/fire/interact. | PASS |
| ALPHA-13: debrief e retorno coletivo | Debrief somente quando todos terminam; Enter retorna todos Waiting/não prontos e preserva secured | `common/src/raid/tests.rs:134` / `:136`: um ativo mantém Active; zero ativos vira Debrief; `server/src/tick/tests.rs:159` a `:172`: extração → debrief → Lobby com secured1; `SCRATCH/server/src/tick/tests.rs:35` a `:41`: um comando retorna os dois jogadores, Waiting, ready=false, carried0, secured3. | PASS |
| ALPHA-13: limpeza e repetição | Caixas reabastecidas na próxima raid, inimigos e projéteis reiniciados | `server/src/tick/tests.rs:173` a `:186`: bots0 no lobby, raid_id2, caixas `[true;3]`, secured1, corpo cheio e bots2; `SCRATCH/server/src/tick/tests.rs:43` / `:44`: projéteis0 e bots0 no retorno; `client/src/playtest.rs:212` a `:223`: segunda raid real com secured1, carried0, caixas repostas, carregador30, dois bots vivos. | PASS |
| ALPHA-13: ninguém conectado | Reseta Lobby e elimina inimigos | `server/src/tick/tests.rs:330` / `:331`: `phase == Lobby`, bots `0`; `E2E/server.log:11` / `:12`: disconnect da segunda raid seguido de Active → Lobby. | PASS |
| ALPHA-14: HUD completo | Fase/tempo, quatro partes, durabilidade, arma/munição/recarga, carried/secured/progresso | `SCRATCH/client/src/hud.rs:184` / `:185`: `text.contains(expected)` para `IN RAID   01:05`, os quatro máximos35/85/60/70, `Vest 60`/`Helmet 40`, `Pistol / Ball`, `17 + 51 reserve [RELOADING]`, `Loot 1`/`Secured 2`, `3.0 / 5.0s`; `E2E/raid.png` e `E2E/extracting.png` confirmam apresentação renderizada, não só string. | PASS |
| ALPHA-14: Escape | Libera e recaptura cursor; ações soltas ao liberar | `client/src/input/mod.rs:266` a `:288`: None no lobby, Locked ao entrar, None e visível após Escape; `:251`: forward/fire/interact/jump desligados. Alternância simétrica verificada na implementação `client/src/input/mod.rs:41` a `:46`. | PASS |
| ALPHA-15: playtest e guia | Executável percorre cliente renderizado e UDP real até retorno/segunda raid; screenshots reais; guia inglês/Mermaid correspondente | `scripts/playtest.py:40` a `:43`, `:56` a `:67`: servidor vivo, exit0 cliente, sem panic/error, marcador PASS, marcos de servidor e cinco PNGs1280×800; `client/src/playtest.rs:201` a `:223`: retorno não pronto, secured1 e segunda raid. Execução independente exit0; `E2E/result.txt:1` e `E2E/client.log:24` registram PASS. Guia em inglês revisado em `docs/GAME_STATE.md:3`, diagrama `:7` a `:30`, controles `:60` a `:76`, limitações `:101` a `:108`. | PASS |
| ALPHA-16: mensagens inválidas e câmera | Ignora serialização inválida e qualquer eixo não finito sem panic | `server/src/tick/tests.rs:228` / `:231` / `:233`: inválido e NaN rejeitados, válido aceito; `common/src/data.rs:54` / `:55`: frame desalinhado decodifica `[1,2,3]`, inválido retorna erro; `SCRATCH/server/src/tick/tests.rs:7` / `:12`: tamanho2049 rejeita; NaN/+∞/−∞ rejeitados nos três eixos. | PASS |
| ALPHA-16: input antigo | Após0.5 s sem novas entradas, neutraliza movimento, tiro e interação | `server/src/tick/tests.rs:242` a `:246`: após80 ticks a128 Hz, `!input.forward`, `!input.fire`, `!input.interact`; limiar0.5 verificado em `server/src/tick/mod.rs:176`. | PASS |

**Resultado da cobertura:** 16/16 IDs verificados; todos os critérios e edge cases listados têm evidência. Nenhuma lacuna de precisão impeditiva foi encontrada na especificação. Testes temporários estão identificados explicitamente, sem atribuí-los à suíte entregue.

## Gates reexecutados pelo Verifier

| Comando na árvore real | Resultado observado |
| --- | --- |
| `cargo check --workspace` | exit0, inclusive recompilação dos três crates após o sensor |
| `cargo build --workspace -j4` | exit0, cliente e servidor recompilados após o sensor |
| `cargo test --workspace -j4` | exit0; 5 cliente +21 common +8 servidor =34 passaram; zero falhas, ignorados ou skips; doc-tests0 |
| `cargo fmt --all -- --check` | exit0, sem saída |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit0, sem warnings |
| `python scripts/playtest.py --no-build --output C:/Users/sandr/AppData/Local/Temp/opencode/extraction-verifier-e2e-20261114` | exit0; novo servidor e cliente renderizado, cinco screenshots válidos e fluxo completo |
| `python C:/Users/sandr/.claude/skills/tlc-spec-driven/scripts/validate_state.py extraction-alpha` | exit0; `0 error(s)` após a escrita deste parecer |

Antes da feature, `git grep` em HEAD não encontrou `#[test]`/`#[cfg(test)]` nos três crates. Após a feature há34 testes. Nenhum teste anterior foi excluído ou enfraquecido. Os12 testes de combate mapeiam ALPHA-04..08; oito de raid mapeiam ALPHA-03/09/11..13; serialização mapeia ALPHA-16; oito ECS mapeiam ALPHA-02..04/06/09..13/16; cinco de cliente mapeiam ALPHA-02/04/14 e feedback de conexão. Não foram encontradas assertions sem relação com o escopo.

**Provas adicionais:** `SCRATCH/independent-probes.log` registra oito testes aprovados, com comando `CARGO_TARGET_DIR=C:/repos/tk/target cargo +nightly test --workspace -j4 verifier_`, executado a partir do scratch. Foram2 de domínio,5 ECS e1 de HUD. Os demais34 foram filtrados nessa execução, não marcados como ignored.

**Cuidado com cache compartilhado:** ao voltar do scratch, Cargo inicialmente reutilizou executáveis de teste temporários (42 testes), embora os fontes reais estivessem intactos. Essa execução não foi contada como evidência dos34 testes entregues. Executei `cargo clean -p common -p client -p server` apenas nos artefatos gerados desses pacotes e repeti testes/build/check/Clippy da árvore real. A execução final recompilou explicitamente `C:/repos/tk/{common,client,server}` e passou nos34 testes originais. Dependências não foram limpas. Não houve compilação concorrente.

## Discrimination sensor

Profundidade: lightweight, três mutações comportamentais de alto risco. Cada uma foi aplicada apenas na cópia isolada, executada e revertida antes da próxima. Nenhuma sobreviveu; todas falharam por assertion, não por compilação.

Prefixo de ambiente para todos os comandos: PowerShell `$env:CARGO_TARGET_DIR='C:/repos/tk/target'`; diretório de execução `SCRATCH/`; toolchain `+nightly` igual ao projeto.

| Mutação e linha na cópia | Comando | Falha observada e log | Resultado |
| --- | --- | --- | --- |
| `SCRATCH/common/src/combat.rs:118`: fração bloqueada `0.2` → `1.0` | `cargo +nightly test --workspace -j4 heavy_armor_stops_low_penetration_and_wears` | exit101; `common/src/combat/tests.rs:60`: `assert_eq!(body.parts[1],75.0)` recebeu35.0, esperado75.0. `SCRATCH/sensor-armor.log`. | KILLED |
| `SCRATCH/common/src/raid.rs:12`: extração5 s →1 s | `cargo +nightly test --workspace -j4 extraction_credits_once_after_five_seconds` | exit101; `common/src/raid/tests.rs:65`: `assert!(!player.tick_extraction(&raid,exit,true,4.0))` falhou porque extraiu antes de5 s. `SCRATCH/sensor-extraction.log`. | KILLED |
| `SCRATCH/common/src/raid.rs:136`: manter caixa disponível (`false` → `true`) | `cargo +nightly test --workspace -j4 loot_requires_live_participant_interaction_and_proximity_and_is_unique` | exit101; `common/src/raid/tests.rs:49`: `assert!(!raid.loot_available[0])` falhou. `SCRATCH/sensor-loot.log`. | KILLED |

**Isolamento confirmado antes de escrever este relatório:** capturei `git status --porcelain=v1` em `SCRATCH/original-status-before.txt` antes das mutações e em `SCRATCH/original-status-after.txt` ao terminar. A comparação literal retornou igualdade. Hashes de `SCRATCH/common/src/combat.rs` e `SCRATCH/common/src/raid.rs` coincidiram com os fontes reais depois de reverter as mutações. Nenhum `git stash`, reset ou checkout foi usado. Scratch preservado intencionalmente no caminho informado.

**Sensor:** 3 injected, 3 killed, 0 survived.

## Playtest real e revisão visual

A execução independente ocorreu entre18:21:15 e18:21:40 UTC conforme os logs:

- `E2E/server.log:3` / `:4`: countdown e início após3 s reais.
- `E2E/server.log:5`: carried1 concedido pelo servidor.
- `E2E/client.log:15`: sentinela morta, munição28; não houve dano direto ou teleporte de teste.
- `E2E/client.log:17` / `:19`: entrada no passo de extração em11.49 s e resultado em16.49 s, coerentes com5 s contínuos. O domínio também verifica o limiar exato, não apenas o tempo aproximado do cliente.
- `E2E/server.log:6` a `:10`: secured1, debrief, Lobby, Countdown e Active na segunda raid.
- `E2E/client.log:24`: marcador final PASS, com as assertions reais de `client/src/playtest.rs` executadas.
- `E2E/server.log:11` / `:12`: desconexão final limpa e servidor retornando ao lobby.

Inspecionei os cinco PNGs tanto em `target/playtest/` (execução anterior) quanto em `E2E/` (nova execução):

| Screenshot | Observação visual independente |
| --- | --- |
| `lobby.png` | Lobby legível com Rifle/AP/Heavy/capacete, roster NOT READY e instruções |
| `raid.png` | Mapa3D renderizado, duas sentinelas, arma, corpo cheio, colete100/capacete40, munição30+90 e contador04:59 |
| `extracting.png` | Círculo verde, caixa coletada cinza, carried1, secured0, progresso1.0/5.0 s, munição28; tórax80/colete90 evidencia dano recebido no combate |
| `debrief.png` | Extracted, secured1, carried0 e instrução de retorno; sem arma ativa |
| `return-lobby.png` | Lobby novamente NOT READY preservando secured1 |

São capturas1280×800, não imagens simuladas. A inspeção visual foi feita pelo Verifier, não foi herdada do autor. Não houve UAT interativo com usuário nesta revisão; o fluxo automatizado e a inspeção gráfica não equivalem a uma avaliação humana de ergonomia.

## Edge cases e qualidade

- [x] Cancelamento durante Countdown, inclusive via sistema ECS, volta ao Lobby sem bots.
- [x] Entrada tardia continua Waiting com colisor desabilitado; ações autoritativas exigem Active.
- [x] Caixa só concede uma unidade, não repete nem concede a outro participante.
- [x] Extração sem loot, morto, Waiting ou fora da raid não concede secured.
- [x] Soltar F, sair horizontalmente ou elevar-se acima da zona reinicia o progresso.
- [x] Limites exatos de2 m de coleta,2.5 m horizontal e2 m vertical da saída exercitados.
- [x] Morte/timeout perdem carried e bloqueiam movimento, tiro e extração; Space não respawna.
- [x] Debrief aguarda todos; comando de um jogador retorna todos sem auto-ready, preserva secured e limpa projéteis/inimigos.
- [x] Servidor vazio volta ao Lobby.
- [x] Pacotes inválidos, oversized e câmera NaN/infinita em todos os eixos são rejeitados.
- [x] Input antigo neutraliza movimento, fire e interact.

Revisão de código: regras compartilhadas continuam em `common`, apresentação/input em `client` e simulação autoritativa em `server`. A serialização usa `common::data`; plugins e módulos focados seguem `AGENTS.md`. Rapier permanece como motor único e o servidor continua headless. Mudanças de dependências, câmera, controles e configuração são relacionadas à migração e ao fluxo entregue. Não foram encontrados serviços externos, persistência ou abstrações genéricas fora do pedido. Os testes verificam valores observáveis da spec, e não apenas a existência de componentes. A limpeza, recarga, penetração e crédito de loot têm assertions concretas.

T1 a T5 estão comprovadas; a condição final de T6 recebe este parecer independente. Os estados Pending na tabela de rastreabilidade da spec e a frase de verificação pendente em tasks.md são registros anteriores a este parecer. Não foram editados por respeito à autorização restrita a validation.md. Este relatório registra ALPHA-01..16 como verificados.

## Limitações e conclusão

- LAN com host remoto não foi testada. O UDP real foi exercitado localmente; concorrência com dois participantes, late join e retorno coletivo foram exercitados em ECS, não em duas máquinas físicas.
- Não houve soak test de300 s no cliente renderizado; o timeout exato foi exercitado no domínio e a integração de resultado no servidor.
- Sem persistência em disco, contas ou autenticação pública, explicitamente fora do escopo. Não são pendências desta entrega.
- Os PNGs não cobrem cada combinação de loadout, cada animação de recarga ou todos os cenários de combate; regras, string do HUD e estados relevantes foram exercitados separadamente.
- As provas adicionais permanecem externas e podem ser incorporadas posteriormente pelo autor, se desejado. Nenhuma alteração em testes reais foi feita nesta revisão.

**Ranked gaps:** nenhum bloqueio funcional reproduzido e nenhum mutante sobrevivente. Não há tarefa de correção decorrente desta verificação. Nenhuma lição foi registrada: não houve falha fundamentada da implementação, lacuna de precisão ou sensor sobrevivente a destilar.

**Conclusão:** pronto para o super alfa local/LAN definido na especificação, dentro das limitações declaradas. PASS não implica prontidão para serviço público, anticheat ou operação persistente.
