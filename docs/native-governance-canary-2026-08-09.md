# Canário da governança nativa do GitHub

Data de execução: 09/08/2026

Esta alteração exclusivamente documental exercita a governança nativa do
repositório sem modificar código, dependências, artefatos ou configuração de
execução do aplicativo.

A evidência válida fica nos eventos e checks do pull request que adiciona este
arquivo. O canário somente é considerado aprovado quando o controlador habilita
o auto-merge para o head exato, a merge queue conclui todos os contextos exigidos
no `merge_group` e o GitHub produz um squash de um único pai, sem bypass
administrativo nem merge direto.
