# Extraction alpha Specification

## Problem Statement

O protótipo conecta jogadores diretamente a uma arena. Falta um ciclo completo de preparação, combate, coleta e saída verificável solo ou em LAN.

## Goals

- Entregar uma partida do lobby à extração, com retorno ao lobby e segunda partida.
- Manter física, equipamento, dano, saque e resultado sob autoridade do servidor.

## Out of Scope

| Feature | Reason |
| --- | --- |
| Contas, matchmaking público, anticheat e autenticação de produção | Super alfa local/LAN usa Netcode Unsecure existente |
| Economia em disco, inventário em grade e assets finais | Equipamento gratuito; saque assegurado apenas na sessão |
| Balística de engenharia, fragmentação e IA tática | Modelo simplificado e sentinelas suficientes para playtest solo |

## Assumptions & Open Questions

| Assumption / decision | Chosen default | Rationale |
| --- | --- | --- |
| Fluxo sem novas decisões do usuário | Executar os planos em sequência sem confirmação intermediária | Pedido explícito de execução autônoma |
| Partida | Todos os clientes prontos iniciam contagem de 3 s, solo permitido; duração 300 s | Ciclo curto e testável |
| Entrada tardia | Aguardar próxima partida sem combater | Não mudar participantes no meio da raid |
| Equipamento | Rifle ou SMG primária, pistola secundária; munição Ball/AP; colete None/Light/Heavy e capacete | Escolhas pequenas com efeito verificável |
| Extração | Segurar F por 5 s no círculo verde, com pelo menos 1 saque | Incentivar entrar no mapa e voltar vivo |
| Morte ou tempo esgotado | Perder saque carregado; sem respawn até próxima raid | Identidade extraction shooter |
| Persistência | Total assegurado dura até desconectar/reiniciar servidor | Não adicionar banco de dados |
| Commits | Não criar commits ou push sem autorização explícita | Preservar trabalho do usuário |
| Falha de transporte | Exibir conexão/erro no cliente, descartar mensagens inválidas e neutralizar entrada antiga | Evitar crash e movimento preso |
| Concorrência e repetição | Ordem fixa no servidor, coleta única por caixa, extração credita uma vez | Resultado determinístico |
| Observabilidade | Logs de transições e playtest reproduzível com screenshots reais | Evidência visível de execução |
| Dependências externas, pagamentos e contas | N/A: sem serviços além da conexão UDP local | Escopo do super alfa |

**Open questions:** none, decisões não discutidas registradas como defaults acima.

## User Stories

### P1: Preparar e iniciar

**Acceptance Criteria**:
1. The workspace SHALL compile with Bevy 0.20.1, bevy_renet2 1.0.0, bevy_rapier3d 0.37.0 and rkyv 0.8.18 (ALPHA-01).
2. WHEN a client connects THEN the game SHALL show a lobby roster, equipment choices and ready state without allowing movement or shooting (ALPHA-02).
3. WHEN all connected clients are ready THEN the server SHALL start a 3 second countdown and initialize a 300 second raid with distinct spawn positions and full equipment (ALPHA-03).
4. IF anyone cancels readiness during countdown THEN the server SHALL return to the lobby without starting the raid (ALPHA-03).
5. WHILE a participant is ready or in a raid the server SHALL reject equipment changes and restrict weapons to the selected primary plus pistol (ALPHA-04).

### P1: Combater e coletar

**Acceptance Criteria**:
1. The game SHALL offer rifle (30 rounds, automatic), SMG (25 rounds, automatic) and pistol (17 rounds, semi automatic), manual timed reload and finite reserve ammo of 3 magazines per weapon (ALPHA-05).
2. WHEN a projectile crosses wood THEN the server SHALL subtract 20 penetration units per metre of thickness and continue only with positive remaining penetration; concrete SHALL stop it (ALPHA-06).
3. WHEN armor stops a head or thorax hit THEN the server SHALL apply 20 percent blunt damage and reduce durability by 40 percent of raw damage; a penetrating hit SHALL apply 80 percent damage (ALPHA-07).
4. The body SHALL start with head 35, thorax 85, arms 60 and legs 70 health; head or thorax at zero SHALL kill, and legs below 35 SHALL reduce movement speed to 55 percent (ALPHA-08).
5. WHEN an active living participant presses F within 2 metres of an available orange crate THEN the server SHALL grant 1 carried loot and mark that crate unavailable for everyone (ALPHA-09).
6. The raid SHALL contain two hostile sentinels that fire at visible living participants, so a solo player can test combat (ALPHA-10).

### P1: Extrair e repetir

**Acceptance Criteria**:
1. WHEN a living participant with loot holds F for 5 continuous seconds within 2.5 horizontal metres and 2 vertical metres of the extraction center THEN the server SHALL mark them Extracted and add carried loot to secured total exactly once (ALPHA-11).
2. IF the participant leaves the zone or releases F THEN extraction progress SHALL reset to zero (ALPHA-11).
3. WHEN a participant dies or the 300 second raid expires THEN the server SHALL clear carried loot, mark Dead or Missing respectively and disallow further movement, shooting or extraction (ALPHA-12).
4. WHEN all raid participants finish THEN the server SHALL enter debrief; WHEN Enter is pressed in debrief THEN the server SHALL return everyone to an unready lobby, preserve secured totals and reset raid-specific loot, enemies and projectiles for the next raid (ALPHA-13).
5. The client SHALL display match phase/time, body health, armor durability, weapon/ammo/reload, carried/secured loot and extraction progress, and SHALL offer Escape to release or recapture the cursor (ALPHA-14).
6. The delivery SHALL include an executable networked playtest that walks a rendered client through lobby, raid, loot, extraction, debrief and return to lobby with screenshots and an English game-state guide with matching Mermaid flow (ALPHA-15).

## Edge Cases

- IF network input has invalid serialization or non-finite camera values THEN the server SHALL ignore it without panic (ALPHA-16).
- IF input stops arriving for 0.5 seconds THEN the server SHALL neutralize held movement, fire and interaction (ALPHA-16).
- IF a client joins an ongoing raid THEN the server SHALL leave them Waiting until the next lobby (ALPHA-03).
- IF no participants remain connected THEN the server SHALL reset to the lobby (ALPHA-13).
- IF a player attempts to extract empty-handed, dead, waiting or outside an active raid THEN the server SHALL grant no secured loot (ALPHA-11).

## Requirement Traceability

| Requirement ID | Story | Phase | Status |
| --- | --- | --- | --- |
| ALPHA-01 | Dependencies | T1 | Verified |
| ALPHA-02 | Lobby | T3/T4/T5 | Verified |
| ALPHA-03 | Start | T3/T4 | Verified |
| ALPHA-04 | Equipment | T2/T4/T5 | Verified |
| ALPHA-05 | Weapons | T2/T4/T5 | Verified |
| ALPHA-06 | Penetration | T2/T4 | Verified |
| ALPHA-07 | Armor | T2/T4 | Verified |
| ALPHA-08 | Body | T2/T4 | Verified |
| ALPHA-09 | Loot | T3/T4/T5 | Verified |
| ALPHA-10 | Solo combat | T4/T5 | Verified |
| ALPHA-11 | Extraction | T3/T4/T5 | Verified |
| ALPHA-12 | Failure | T3/T4 | Verified |
| ALPHA-13 | Repeat | T3/T4 | Verified |
| ALPHA-14 | HUD | T5 | Verified |
| ALPHA-15 | Complete playtest | T6 | Verified |
| ALPHA-16 | Input safety | T4 | Verified |

## Success Criteria

- `cargo check`, `cargo build`, `cargo test`, formatting and relevant Clippy checks pass.
- Rendered client and real UDP server complete the playtest without panic.
- Independent verification cites concrete test assertions and outcomes.
