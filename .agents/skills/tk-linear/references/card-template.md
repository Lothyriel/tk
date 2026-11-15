# Card template

Write for people in the user's language. Omit optional empty sections. A card is ready when its outcome, boundaries and observable acceptance are clear and blocking product questions are resolved.

Set metadata through MCP, not just prose: team Tarkov-killer, project TK (`tk`, `P-TAR-1`), at least one relevant available label, and explicit priority (High=2, Medium=3, Low=4; Urgent=1 only for an immediate incident/explicit direction). Apply this to parent and child cards too.

```markdown
## Objetivo
Problema atual e resultado desejado.

## Comportamento esperado
O que o jogador ou sistema deve observar, incluindo os casos importantes.

## Escopo
O que entra e os limites relevantes.

## Critérios de aceite
- Resultados observáveis, com condições e limites quando necessários.
- Incluir regressões relevantes e distinguir aceite técnico de julgamento artístico.

## Verificação
Como demonstrar os critérios: testes, medições, rede real ou playtest pertinente.

## Prioridade
Motivo breve da prioridade escolhida, baseado em impacto e dependências.

## Dúvidas e dependências
Somente escolhas ainda abertas e cards que bloqueiam o trabalho.

+++ Agent notes

Descobertas do código com caminhos, alternativas e motivos, restrições técnicas
e sugestões de implementação. Separar fatos verificados de hipóteses.

+++
```

For experiments, state the hypothesis, bounded experiment, observation and decision needed. Completing an experiment need not imply shipping the attempted design.

Suggested progress comment (adapt, don't fill empty fields):

```markdown
**Resultado/decisão:** ...
**Motivo:** ...
**Evidência:** branch, commit, comando/cenário e resultado.
**Pendente/próximo passo:** ...
```
