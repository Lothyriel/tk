# Desenvolvimento com Linear

## Sonda opt-in da grade de inventário

Após coordenação, inicie o servidor local normalmente e rode
`cargo run --bin client -- --inventory-grid-probe`, sem outros modos de
playtest/review. Entre em uma raid normal, colete a garrafa de água (1×2) e,
para testar pilhas, colete a pilha de duas bandagens.
Deixe espaço para mover a garrafa girada e, para split, uma célula livre.
Abra a mochila com Tab e mantenha o cliente em foco, sem interagir.

A sonda usa eventos sintéticos de teclado no fluxo Enter/setas/R da grade
real. Espera o feedback reconciliado e a revisão confirmada pelo servidor
para cada operação. Procure `GRID PROBE PASS move/rotate and split/merge` ou
`GRID PROBE PASS move/rotate only`; no segundo caso o log marca split/merge
como não exercitados. A sonda não semeia itens, não simula confirmação e
não faz capturas. Não certifica mouse, teclado físico, legibilidade nem perda
de pacotes.

Com a pilha de duas bandagens, rode
`python scripts/playtest.py --voxel-loot --inventory-grid --no-capture`
com um cliente. A rota coleta as bandagens e a água no servidor, abre a grade
com Tab, confirma rotação/movimento e split/merge, fecha com Tab, descarta os
dois IDs originais e segue até a segunda raid. Sem a pilha real ou sem
confirmação da grade, a rota falha.

## Sonda opt-in do menu Arena

Na revisão integrada com Arena, inicie um servidor local novo, sem outros
clientes ou fixtures. Após coordenar a execução renderizada, inicie
`cargo run --bin client -- --arena-menu-probe=mouse`. Repita com
`--arena-menu-probe=keyboard` e `--arena-menu-probe=mixed`, com servidor novo
para cada percurso. Mantenha a janela em foco sem interagir durante a sonda.
Não combine com `--playtest`.

A sonda visita a bancada, confirma e cancela prontidão, entra na Arena Solo,
espera a eliminação pelo bot, volta ao lobby e repete a preparação e entrada.
Procure `ARENA MENU PROBE PASS` no log. São eventos sintéticos do Bevy,
incluindo picking direcionado a widgets com layout; não certificam eventos do
SO, hit-testing, legibilidade, nem perda UDP. A sonda não registra mídia.

## Revisão estática da grade

Somente após coordenação, com um servidor normal e loot real, rode o cliente
sem `--playtest`:

```sh
TK_GRID_REVIEW_DIR="$HOME/tmp/tar56-grid-720" TK_PLAYTEST_VIEWPORT=1280x720 \
  cargo run --locked -p client -- --inventory-grid-review
```

Use um diretório novo por execução e repita em `1280x800` e `1920x1080`.
Repita com `TK_GRID_REVIEW_UI_SCALE=1.25` para verificar escala maior.
Confirme a resolução física no log e nas imagens; não conte uma janela que
o compositor tenha redimensionado como parte da matriz.

Entre na raid, colete a água e as bandagens, abra a grade apontando fora dela,
passe o cursor sobre um item, arraste para uma célula válida e outra inválida
(por exemplo, a última coluna com a garrafa girada), solte numa posição válida
para enviar o movimento e abra o menu contextual com clique direito ou F10.
O observador registra um PNG por estado e `GRID REVIEW` com revisão, escala
e limites de layout. Aguarde os arquivos antes de fechar o cliente. Se não
houver um frame real de pending ou o arquivo não for produzido, deixe a
evidência pendente; não congele o estado, simule confirmação ou atrase o
servidor para obter a imagem. `inside` não substitui revisão de legibilidade,
foco, sobreposição ou áudio. Não há injeção de input nem vídeo neste modo.

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

No `stop-lean`, use `--clients 2` para capturar os extremos e o retorno
também no observador, que descarta cada terceiro snapshot. Use `--pov
side-cover` para conferir as duas paredes no cliente solo. `--viewport
1260x540` exige a dimensão exata dos PNGs, sem alterar o FOV. Com
`--pov-hz 30` ou `--pov-hz 165`, `stop-lean` grava a posição do olho, o
roll, a cadência efetiva e a projeção normalizada do marcador em
`pov-trace.csv`, sem vídeo. A projeção não mede oclusão. Compare também a
posição de parada: percorrer a mesma rota não garante a mesma trajetória.

Para revisar a postura voluntária, após coordenar o uso de GPU/UDP, use
`python scripts/playtest.py --no-build --pov prone --clients 2 --viewport 900x700 --output target/pov-prone`.
O líder deita, rasteja sob uma cobertura baixa e tenta levantar. Depois de
confirmar que permaneceu deitado, sai rastejando e envia uma nova alternância.
O observador descarta cada terceiro snapshot e exige pose confirmada e altura
do tronco compatíveis nos checkpoints `prone`, `refused` e `return`. Compare
os logs e capturas: a geometria da cobertura pode ocultar parte da silhueta.
O preset não testa queda por lesão nem certifica naturalidade em movimento.

Para o tropeço, use `python scripts/playtest.py --no-build --pov stumble
--clients 2 --viewport 900x700 --output target/pov-stumble` após coordenar a
execução. O líder corre contra um obstáculo baixo, mantém a corrida durante
a retomada sem rearmar o contato e recua em pé. O observador descarta cada
terceiro snapshot e confirma tropeço, retorno em contato e recuo na ordem.
O preset não substitui a matriz física de caminhada, degrau e parede.

Para a falha fictícia, após coordenação, use `python scripts/playtest.py
--no-build --pov action-fault --clients 2 --output target/pov-action-fault`
com ambos os binários da mesma revisão. O preset habilita a fixture privada
e compara bloqueio, interrupção, persistência e recuperação entre os clientes.
O peer descarta cada terceiro snapshot. Os logs devem mostrar um único cue
em cada vista, com saída de áudio desativada. Isso não verifica reprodução
audível, reconexão ou entrada tardia.

No relatório `observer_gait` em `run-settings.json`, compare os picos por
quadro com `peak_support_excursion`: o maior deslocamento desde o primeiro
quadro de um trecho continuamente carregado (peso > 0,95, pelo menos dois
pares, sem lacunas de 50 ms ou mudança de faixa de velocidade). O registro
identifica pé, timestamps e snapshots; os deslocamentos das âncoras apresentada
e confirmada usam os mesmos extremos que o pé, e não picos independentes.
`null` indica que não houve trecho longo o suficiente. A métrica ajuda a
localizar deriva lenta, mas não substitui revisão em velocidade normal.

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
