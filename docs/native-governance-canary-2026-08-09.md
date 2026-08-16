# Canário da governança nativa do GitHub

Data de execução: 09/08/2026

Esta alteração exclusivamente documental exercita a governança nativa do
repositório sem modificar código, dependências, artefatos ou configuração de
execução do aplicativo.

A evidência válida ficou nos eventos e checks do pull request que adicionou este
arquivo. Naquele ciclo, um controlador ainda habilitava o auto-merge para o head
exato. Esse componente foi posteriormente aposentado: a admissão agora é humana
e explícita, e a merge queue continua responsável pelos contextos do
`merge_group` e pelo squash de um único pai, sem bypass administrativo nem merge
direto.
