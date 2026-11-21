# Desenvolvimento com Linear

## Entradas compartilhadas

OpenCode e Codex descobrem skills do projeto em `.agents/skills/`.
As três skills de procedimento são `tk-linear`, `tk-development` e `tk-verification`.
`planejar` e `implementar` são entradas curtas que reutilizam esses procedimentos.

| Ação | OpenCode | Codex |
| --- | --- | --- |
| Investigar e preparar cards | `/planejar sua proposta` | `$planejar sua proposta` ou selecionar em `/skills` |
| Listar cards ativos e escolher | `/implementar` | `$implementar` |
| Implementar um card específico | `/implementar TAR-123` | `$implementar TAR-123` |

O diretório compartilhado é de **skills**, não de slash commands universais.
Os atalhos de barra do OpenCode ficam em `.opencode/commands/` e apenas encaminham os argumentos.
No Codex, use a invocação de skill documentada. Reinicie/reabra a sessão após instalar novas skills se a lista ainda estiver antiga.

## Linear MCP

- OpenCode: configuração local em `opencode.json`.
- Codex: configuração local em `.codex/config.toml`, carregada para projetos confiáveis. Autentique com `codex mcp login linear`; a autenticação do OpenCode não implica autenticação do Codex.
- Endpoint: `https://mcp.linear.app/mcp`. Credenciais ficam no armazenamento do harness, nunca no repositório.
- Equipe: **Tarkov-killer**, prefixo **TAR**. O agente consulta schemas/statuses pelo MCP e usa paginação.
- Novos cards (incluindo pais e subtarefas): projeto **TK** (`tk`, `P-TAR-1`), labels pertinentes já disponíveis e prioridade explícita. High para bloqueios/alto impacto, Medium para trabalho normal, Low para polimento opcional; Urgent fica reservado a incidentes imediatos ou orientação explícita. O planejamento registra o motivo.
- Se o MCP estiver indisponível, o agente informa o bloqueio. Um rascunho no chat não é um card salvo.

## Como trabalhar

Planejamento investiga código, testes, cards existentes e alternativas antes de criar ou atualizar cards.
Termina com links e decisões pendentes, sem começar a implementação.
Um resultado coeso cabe em um card; entregas independentes podem ser vários cards, ou subtarefas com critérios próprios sob um pai com aceite de integração.

Implementação sem ID lista todos os cards abertos da equipe, incluindo Backlog, Todo e In Progress, e aguarda a escolha.
Com ID, lê descrição completa, comentários, dependências e código antes de agir.
Ao iniciar a tarefa escolhida e pronta, atribui o card ao usuário autenticado no MCP (`assignee: "me"`) e muda para In Progress. Listar ou planejar não altera o responsável; selecionar uma subtarefa não reatribui toda a família.
Hotfix pequeno e localizado pode partir do chat. Mudança grande precisa de card com objetivo, escopo, critérios verificáveis e escolhas bloqueantes resolvidas; se o trabalho crescer, o agente volta ao planejamento.

A descrição guarda o acordo atual; comentários registram descobertas, motivos, alternativas rejeitadas, bloqueios, entregas verificadas e retomada.
Git guarda a implementação. Documentação técnica guarda decisões reutilizáveis. Não há um diário operacional paralelo no repositório.

O desenvolvimento é trunk-based: implementação e commits verificados são feitos diretamente em `main`, com push para `origin/main`.
Commits automáticos em inglês seguem Conventional Commits. Branches de tarefa só são criadas quando solicitadas explicitamente.
Pedidos explícitos de trabalho local prevalecem. PR, release e deploy são ações separadas.
O agente preserva alterações locais alheias e os hooks do Git.

## Verificação

Use o toolchain de `rust-toolchain.toml`, o `Cargo.lock` versionado e Python 3.12+.
Em Linux x86_64, instale `clang` e `/usr/bin/mold`.

```sh
python scripts/verify.py
python scripts/verify.py --package client --filter local_camera_motion
python scripts/verify.py --package server --filter real_rapier
python scripts/verify.py --mode full
python scripts/verify.py --mode docs
```

Quick cobre tooling e testes common/server; package/filter permite foco, inclusive cliente sem janela.
Full inclui fmt, check, Clippy com warnings negados e testes do workspace. Docs cobre diff e testes Python, sem Cargo.
Todos param na primeira falha; nenhum abre jogo, rede real ou grava vídeo. Uma seleção sem testes executados não prova nada.
Testes headless não substituem aceite perceptual ou transporte real. Consulte `tk-verification` para escolher o cenário mínimo pertinente.

Os scripts de playtest coordenam processos reais e preservam evidências. `validate_humanoids.py` reutiliza o gate central antes dos cenários solo/dual; as regras de fmt/check/Clippy/testes ficam apenas em `verify.py`.
Scripts de baseline e comparação preservam proveniência e verificações específicas de formatos históricos. Use-os quando o aceite exigir comparação, não como etapas obrigatórias de toda tarefa.

### Bancada curta de POV

```sh
cargo build --locked -p client -p server
python scripts/playtest.py --no-build --weapon-kit compact --pov stop-lean --output target/pov-stop-lean
python scripts/verify.py --package client --filter playtest::pov
```

Use `--pov ads` para mirar e soltar, ou `--pov inspect` para confirmar início e
retorno da inspeção. O preset `stop-lean` caminha, corre e freia diante da
barreira baixa de concreto, agacha e inclina para ambos os lados. Os comandos
passam pelo cliente e servidor UDP normais. Somente com `--pov`, o cenário
inclui um degrau estático e um marcador fixo na barreira, sem NPCs que possam
alterar a pose por dano. A bancada solo não substitui o observador remoto nem
o gate de rede.

Compare capturas de diretórios separados com o mesmo kit e viewport. Confira
`run-settings.json` para hashes dos binários e projeções e `client.log` para
estado confirmado por captura. Se a revisão não tiver animação de inspeção,
o preset ainda verifica o estado, não a pose. Para medir tempo de iteração,
alterne três execuções quentes com e sem `--pov`, mantendo os mesmos binários
e medindo o tempo de parede até o processo terminar. Isso não é benchmark
de frame time nem valida naturalidade em movimento.

`docs/GAME_STATE.md` é o guia do jogador: só muda quando execução, controles, gameplay ou limitações relevantes mudam.

## Referências dos harnesses

- [Skills OpenCode](https://opencode.ai/docs/skills)
- [Comandos OpenCode](https://opencode.ai/docs/commands)
- [Skills Codex](https://developers.openai.com/codex/skills/)
- [MCP Codex](https://developers.openai.com/codex/mcp/)
