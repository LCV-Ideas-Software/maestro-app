#!/usr/bin/env node
// Monta THIRD-PARTY-NOTICES.txt com o texto integral de licenca de cada
// componente incorporado ao executavel distribuido.
//
// Fecha em falha. Se qualquer componente distribuido ficar sem texto de
// licenca, o processo termina com codigo diferente de zero e lista os
// faltantes; nunca emite um arquivo com aviso vazio.
//
// Fontes de identidade, ambas oficiais e ja usadas pelo gate de inventario:
//   npm   -> package-lock.json, excluindo entradas marcadas `dev`
//   cargo -> cargo metadata --locked --filter-platform <alvo>, seguindo apenas
//            arestas de dependencia normal a partir da raiz
//
// Fonte do texto: o proprio artefato baixado. Quando o publicador nao inclui o
// texto, vale o fragmento vendorizado declarado em scripts/legal/thirdparty-policy.mjs,
// cujo sha256 e conferido aqui.
//
// Uso:
//   node scripts/generate-notices.mjs            grava o arquivo
//   node scripts/generate-notices.mjs --check    nao grava; falha se divergir

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Implementacao de referencia da jslicense, a mesma que o npm usa para validar
// o campo `license`. Dependencia de desenvolvimento: roda no gate, nao vai
// para o executavel distribuido.
import spdxParse from "spdx-expression-parse";

import { POLICY } from "./legal/thirdparty-policy.mjs";

const RAIZ = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const MODO_CHECK = process.argv.includes("--check");
const SAIDA = resolve(RAIZ, POLICY.outputs.notices);

const sha256 = (t) => createHash("sha256").update(t, "utf8").digest("hex");
const paraLf = (t) => t.split("\r\n").join("\n");

function falhar(titulo, linhas) {
  console.error(`\n${titulo}\n`);
  for (const l of linhas) console.error(`  ${l}`);
  console.error("");
  process.exit(1);
}

// ---------------------------------------------------------------- fragmentos

const fragmentos = new Map();
{
  const divergentes = [];
  for (const [chave, frag] of Object.entries(POLICY.fragments)) {
    const caminho = resolve(RAIZ, frag.path);
    if (!existsSync(caminho)) {
      divergentes.push(`${chave}: arquivo ausente em ${frag.path}`);
      continue;
    }
    const texto = readFileSync(caminho, "utf8");
    const h = sha256(texto);
    if (h !== frag.sha256) {
      divergentes.push(`${chave}: sha256 ${h} nao confere com ${frag.sha256} declarado`);
      continue;
    }
    fragmentos.set(chave, texto);
  }
  if (divergentes.length) {
    falhar("Fragmentos de licenca divergem da politica declarada:", divergentes);
  }
}

// ---------------------------------------------------------------------- npm

function nomeDaChaveDoLock(chave) {
  const marcador = "node_modules/";
  const i = chave.lastIndexOf(marcador);
  return i === -1 ? chave : chave.slice(i + marcador.length);
}

// O npm aninha um pacote sob outro quando ha conflito de versao, e nesse caso
// o caminho declarado no lockfile nao existe em disco. Em vez de adivinhar o
// aninhamento, indexa-se a arvore instalada uma unica vez por nome e versao
// lidos do package.json de cada pacote, que e a fonte autoritativa.
let indiceNpm = null;

function construirIndiceNpm() {
  const indice = new Map();
  const pilha = [resolve(RAIZ, "node_modules")];
  while (pilha.length) {
    const dir = pilha.pop();
    let entradas;
    try {
      entradas = readdirSync(dir, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const e of entradas) {
      if (!e.isDirectory() || e.name === ".bin") continue;
      const p = join(dir, e.name);
      if (e.name.startsWith("@")) {
        pilha.push(p);
        continue;
      }
      try {
        const j = JSON.parse(readFileSync(join(p, "package.json"), "utf8"));
        if (j.name && j.version) {
          const id = `${j.name}@${j.version}`;
          if (!indice.has(id)) indice.set(id, p);
        }
      } catch {
        /* sem package.json legivel: nao e um pacote, segue */
      }
      pilha.push(join(p, "node_modules"));
    }
  }
  return indice;
}

function acharDiretorioNpm(chaveDoLock, nome, versao) {
  const direto = resolve(RAIZ, chaveDoLock);
  if (existsSync(direto)) return direto;
  if (!indiceNpm) indiceNpm = construirIndiceNpm();
  return indiceNpm.get(`${nome}@${versao}`) || null;
}

function textoDeLicencaNoDiretorio(dir) {
  if (!dir || !existsSync(dir)) return null;
  let entradas;
  try {
    entradas = readdirSync(dir);
  } catch {
    return null;
  }
  const util = (nome) =>
    !POLICY.licenseFileIgnoredExtensions.some((ext) =>
      nome.toLowerCase().endsWith(ext),
    );
  const comecaCom = (nome, lista) =>
    lista.some((p) => nome.toLowerCase().startsWith(p));

  const portadores = entradas.filter(
    (e) => util(e) && comecaCom(e, POLICY.licenseFilePrefixes),
  );
  // Um NOTICE isolado nao satisfaz a exigencia: e material suplementar, nao o
  // texto da licenca. Sem arquivo portador, o componente cai em `semTexto` e o
  // gate reprova, em vez de emitir um pacote de avisos incompleto.
  if (!portadores.length) return null;

  const suplementares = entradas.filter(
    (e) => util(e) && comecaCom(e, POLICY.supplementalFilePrefixes),
  );
  const achados = [...portadores, ...suplementares];
  const nomesPortadores = new Set(portadores);

  const partes = [];
  let portadorLido = false;
  for (const a of achados.sort()) {
    // Le direto em vez de checar o tipo antes: consultar e depois usar deixa
    // uma janela entre as duas chamadas. Um diretorio faz readFileSync lancar
    // EISDIR, que o catch trata, com o mesmo efeito e sem a janela.
    try {
      const t = paraLf(readFileSync(join(dir, a), "utf8")).trim();
      if (!t) continue;
      partes.push({ arquivo: a, texto: t });
      if (nomesPortadores.has(a)) portadorLido = true;
    } catch {
      /* nao e arquivo legivel: segue */
    }
  }
  // Existir um nome portador nao basta: ele pode ser um diretorio `LICENSES/`
  // ou um arquivo vazio, e nesse caso so sobraria material suplementar. So
  // conta como coberto quando ao menos um portador rendeu texto de verdade.
  return portadorLido ? partes : null;
}

// Componentes deixados de fora por restricao de plataforma, reportados no
// cabecalho do arquivo gerado.
let plataformaExcluidos = [];

// npm documenta `os`, `cpu` e `libc` como restricoes de plataforma. Um pacote
// restrito a outra plataforma nao e instalado e nao pode estar no artefato, e
// exigi-lo faria o gate reprovar por uma ausencia legitima.
//
// A referencia e a plataforma do ARTEFATO declarada na politica, nao a da
// maquina que executa: filtrar pelo host faria o conjunto de avisos mudar
// conforme onde o comando roda.
function plataformaExcluida(meta) {
  const casa = (lista, atual) => {
    if (!Array.isArray(lista) || !lista.length) return true;
    if (atual === null || atual === undefined) return true;
    const negados = lista.filter((v) => v.startsWith("!")).map((v) => v.slice(1));
    const permitidos = lista.filter((v) => !v.startsWith("!"));
    if (negados.includes(atual)) return false;
    return permitidos.length === 0 || permitidos.includes(atual);
  };
  const alvo = POLICY.scope.npm;
  return (
    !casa(meta.os, alvo.targetOs) ||
    !casa(meta.cpu, alvo.targetCpu) ||
    !casa(meta.libc, alvo.targetLibc)
  );
}

function componentesNpm() {
  const lock = JSON.parse(readFileSync(resolve(RAIZ, "package-lock.json"), "utf8"));

  // Um lockfile v1 guarda a arvore em `dependencies`, nao em `packages`. Tratar
  // o indice ausente como conjunto vazio aprovaria um arquivo de avisos com
  // zero componentes npm, que e o pior resultado possivel para um gate que
  // existe para fechar em falha.
  if (!lock.packages || Object.keys(lock.packages).length === 0) {
    falhar("Formato de lockfile nao suportado:", [
      `package-lock.json: lockfileVersion=${lock.lockfileVersion ?? "ausente"} sem indice \`packages\``,
      "Este gate exige lockfileVersion 2 ou superior. Rode: npm install",
    ]);
  }

  const raizLock = lock.packages[""] || {};

  // O alcance de um link de workspace nao se le na entrada dele: o npm nao lhe
  // poe a marcacao `dev` mesmo quando a raiz so o declara em devDependencies, e
  // a entrada-alvo tampouco recupera essa informacao. As secoes da raiz cobrem
  // o link declarado ali diretamente, mas nao um pacote alcancavel apenas por
  // baixo de um workspace de ferramenta: para esse, seria preciso caminhar o
  // grafo a partir das raizes de producao.
  //
  // Nenhum dos repositorios da frota declara `workspaces` hoje, entao esse
  // caminho nao existe. Em vez de escrever a travessia para uma forma que nao
  // ha — e nao teria como ser provada contra nada —, o gate para se alguem
  // declarar um workspace. A travessia passa a ser exigida no momento em que
  // ela deixa de ser especulativa.
  if (Array.isArray(raizLock.workspaces) && raizLock.workspaces.length) {
    falhar("Escopo de workspace nao coberto por este gate:", [
      `package-lock.json declara workspaces: ${raizLock.workspaces.join(", ")}`,
      "Um pacote alcancavel so por baixo de um workspace de desenvolvimento nao",
      "recebe marcacao `dev` e seria publicado como componente distribuido.",
      "Derive o alcance a partir das dependencias de producao da raiz antes de",
      "voltar a gerar os avisos.",
    ]);
  }

  const producaoNaRaiz = new Set([
    ...Object.keys(raizLock.dependencies || {}),
    ...Object.keys(raizLock.optionalDependencies || {}),
  ]);
  const devNaRaiz = new Set(Object.keys(raizLock.devDependencies || {}));

  const marcador = POLICY.scope.npm.excludeDevMarker;
  const saida = [];
  const vistos = new Set();
  const naoResolvidos = [];
  const excluidosPorPlataforma = [];
  for (const [chave, metaOriginal] of Object.entries(lock.packages)) {
    if (!chave.startsWith("node_modules/")) continue;
    if (metaOriginal[marcador] === true) continue;

    // Um link de workspace nao recebe a marcacao `dev` mesmo quando a raiz so o
    // declara em devDependencies, e o alvo resolvido tampouco recupera essa
    // informacao. O alcance vem, entao, das secoes da raiz.
    if (metaOriginal.link === true) {
      const nomeLink = nomeDaChaveDoLock(chave);
      if (devNaRaiz.has(nomeLink) && !producaoNaRaiz.has(nomeLink)) continue;
    }

    if (plataformaExcluida(metaOriginal)) {
      excluidosPorPlataforma.push(nomeDaChaveDoLock(chave));
      continue;
    }

    // Uma entrada com `link: true` (dependencia `file:` ou de workspace) nao
    // carrega versao nem licenca: esses metadados vivem na entrada alvo. Pular
    // por ausencia de versao faria o componente sumir dos avisos sem que o
    // gate reclamasse, que e exatamente o oposto de fechar em falha.
    let meta = metaOriginal;
    if (metaOriginal.link === true) {
      const alvo = metaOriginal.resolved
        ? lock.packages[metaOriginal.resolved]
        : null;
      if (!alvo) {
        naoResolvidos.push(
          `${chave}: entrada com link nao resolvida (resolved=${metaOriginal.resolved ?? "ausente"})`,
        );
        continue;
      }
      meta = { ...alvo, name: alvo.name || metaOriginal.name };
    }

    const nome = meta.name || nomeDaChaveDoLock(chave);
    const versao = meta.version;
    if (!versao) {
      naoResolvidos.push(`${chave}: sem versao no lockfile`);
      continue;
    }
    const id = `${nome}@${versao}`;
    // Nome e versao nao identificam um artefato. Um fork em git, um `file:` ou
    // outro registro pode preservar as duas coordenadas e ainda assim ser outro
    // codigo, com outra licenca — e os dois seriam instalados em caminhos
    // diferentes e empacotados juntos. Deduplicar so pelas coordenadas
    // descartava o segundo antes de olhar o diretorio dele. A identidade passa
    // a incluir a origem que o lockfile resolve, e so ela: `resolved`
    // discrimina exatamente o caso — fork em git traz `git+https://...#sha`,
    // `file:` traz o caminho, tarball traz a URL do registro. `integrity` nao
    // acrescenta discriminacao e acrescenta risco: o npm ja gravou sha1 e
    // sha512, e omite o campo em algumas entradas-alvo, o que partiria UM
    // artefato em dois blocos de cabecalho identico. Copia hasteada e copia
    // aninhada do MESMO artefato continuam colapsando.
    // Num link, o discriminante e o caminho para onde ELE aponta, nao a origem
    // do alvo: a entrada-alvo de um pacote de workspace nao tem `resolved`, e
    // dois links para alvos diferentes de mesmo nome e versao colapsariam num
    // componente so. Fora de link, `metaOriginal` e o proprio `meta`.
    const origemDaIdentidade =
      metaOriginal.link === true ? metaOriginal.resolved : meta.resolved;
    const identidade = `${id}|${origemDaIdentidade ?? ""}`;
    if (vistos.has(identidade)) continue;
    vistos.add(identidade);
    saida.push({
      ecossistema: "npm",
      nome,
      versao,
      id,
      licencaDeclarada: meta.license || null,
      // De onde o pacote veio de fato. Um fallback so pode valer para o
      // artefato do registro canonico: trocar a dependencia por um git, um
      // `file:` ou outro registro mantendo nome e versao nao pode herdar a
      // proveniencia travada de outro pacote.
      origemPacote: meta.resolved || null,
      diretorio: acharDiretorioNpm(chave, nome, versao),
    });
  }
  if (naoResolvidos.length) {
    falhar(
      "Entradas do lockfile que nao puderam ser resolvidas. Um componente distribuido nao pode ficar fora dos avisos em silencio:",
      naoResolvidos,
    );
  }
  // Registrado, nao silenciado: quem auditar precisa saber que houve exclusao
  // por plataforma e quais foram.
  plataformaExcluidos = [...new Set(excluidosPorPlataforma)].sort();
  return saida.sort((a, b) => a.id.localeCompare(b.id));
}

// ------------------------------------------------------------------ eleicao

// Quando um componente oferece mais de uma licenca, e a eleicao que determina
// as obrigacoes assumidas.
//
// A expressao NAO e interpretada aqui a mao. Ela e analisada por
// `spdx-expression-parse`, a implementacao de referencia da jslicense, a mesma
// que o npm usa para validar o campo `license`. Tres rodadas de revisao sobre
// divisao de string por `OR`/`AND` mostraram o que a diretriz da frota ja dizia:
// gramatica de especificacao nao se implementa a mao. Com a arvore em maos, o
// que antes era heuristica vira calculo exato.
const normalizarBarraLegada = (expressao) =>
  expressao.includes("/") ? expressao.split("/").map((t) => t.trim()).join(" OR ") : expressao;

// A forma legada do Cargo (`MIT/Apache-2.0`) e uma disjuncao e o proprio Cargo
// a documenta como equivalente a `OR`, mas nao e SPDX valido. Normaliza-se
// antes de analisar. Nao ha excecao para URL: uma URL tambem nao e expressao
// SPDX valida e cai na mesma falha de analise, que e o tratamento correto.
function analisarExpressao(expressao) {
  try {
    return { ast: spdxParse(normalizarBarraLegada(expressao.trim())) };
  } catch (erro) {
    return { erro: erro.message };
  }
}

// Uma folha e uma licenca — com a excecao acoplada, quando ha `WITH`, porque
// `Apache-2.0 WITH LLVM-exception` e uma unica licenca efetiva, nao duas.
const folhaComoTexto = (no) =>
  no.exception ? `${no.license} WITH ${no.exception}` : no.license;

function folhasDaExpressao(no) {
  if (no.license) return [folhaComoTexto(no)];
  return [...folhasDaExpressao(no.left), ...folhasDaExpressao(no.right)];
}

// Obrigatoria e a licenca presente em TODA atribuicao que satisfaz a expressao.
// Uniao sob `AND`, intersecao sob `OR`. Isso resolve exatamente o caso misto
// `(MIT OR Apache-2.0) AND Unicode-3.0`, em que a Unicode-3.0 e obrigatoria e a
// escolha entre as outras duas e livre — que a divisao por string nao decidia.
function licencasObrigatorias(no) {
  if (no.license) return new Set([folhaComoTexto(no)]);
  const esquerda = licencasObrigatorias(no.left);
  const direita = licencasObrigatorias(no.right);
  return no.conjunction === "and"
    ? new Set([...esquerda, ...direita])
    : new Set([...esquerda].filter((l) => direita.has(l)));
}

// Um conjunto de licencas satisfaz a expressao? Esta unica pergunta substitui
// as tres conferencias anteriores: que a eleita e oferecida, que ela cobre todo
// termo obrigatorio, e que a escolha e legitima. Uma atribuicao que satisfaz a
// expressao contem os obrigatorios por construcao.
function satisfaz(no, escolhidas) {
  if (no.license) return escolhidas.has(folhaComoTexto(no));
  return no.conjunction === "and"
    ? satisfaz(no.left, escolhidas) && satisfaz(no.right, escolhidas)
    : satisfaz(no.left, escolhidas) || satisfaz(no.right, escolhidas);
}

// A licenca eleita precisa estar efetivamente reproduzida no artefato. Sem
// isso, o arquivo pode afirmar Apache-2.0 enquanto reproduz o texto da CC0.
//
// O texto das licencas vem quebrado em larguras diferentes conforme o pacote: o
// LICENSE-MIT do unicode-ident quebra "this permission notice / shall be
// included" no meio da frase. A caixa tambem varia entre o marcador declarado e
// o texto canonico, e ha pacote que publica a licenca inteira com "// " na
// frente de cada linha, por ser copiada do cabecalho do fonte. As tres sao
// diferencas tipograficas, nao juridicas, e sao normalizadas dos dois lados
// antes de comparar. A normalizacao so remove; nunca insere texto que o arquivo
// nao tenha.
const normalizarParaComparar = (t) =>
  t
    .split("\n")
    .map((l) => l.replace(/^\s*(?:\/\/+|#+|\*+)\s?/u, ""))
    .join("\n")
    .replace(/\s+/gu, " ")
    .trim()
    .toLowerCase();

function corroboradas(licencas, textos) {
  const corpo = normalizarParaComparar(textos.map((t) => t.texto).join("\n"));
  for (const termo of licencas) {
    const marcadores = POLICY.licenseTextMarkers[termo];
    if (!marcadores) return { ok: false, motivo: `sem marcador declarado para ${termo}` };
    if (!marcadores.some((m) => corpo.includes(normalizarParaComparar(m)))) {
      return {
        ok: false,
        motivo: `nenhum marcador de ${termo} aparece no texto reproduzido`,
      };
    }
  }
  return { ok: true };
}

// A eleicao registrada e escrita como expressao (`MIT AND Unicode-3.0`); as
// licencas efetivamente assumidas sao as folhas dela.
function licencasDaEleicao(elected) {
  const { ast, erro } = analisarExpressao(elected);
  return erro ? { erro } : { licencas: new Set(folhasDaExpressao(ast)) };
}

function elegerLicencas(componentes) {
  const pendentes = [];
  for (const c of componentes) {
    const expressao = (c.licencaDeclarada || "").trim();
    if (!expressao) continue;

    const { ast, erro } = analisarExpressao(expressao);
    if (erro) {
      // Declaracao que nao e expressao SPDX valida — "BSD" solto, uma URL — nao
      // tem como ser conferida. Ela nao passa por omissao: ou ha inspecao
      // manual registrada, ou o gate para. A conferencia do texto reproduzido
      // desses casos fica com `corroborarLicencaUnica`.
      if (!POLICY.unverifiableLicenseDeclarations?.[c.id]) {
        pendentes.push(
          `${c.id}: "${expressao}" nao e expressao SPDX valida (${erro}); registre a inspecao manual em unverifiableLicenseDeclarations`,
        );
      }
      continue;
    }

    const folhas = folhasDaExpressao(ast);
    // Uma folha so: nao ha escolha a fazer nem obrigacao a somar. A
    // corroboracao desse caso e feita por `corroborarLicencaUnica`, que cobre
    // tambem os componentes sem nenhuma expressao composta.
    if (folhas.length === 1) continue;

    const explicita = POLICY.licenseElections[c.id];
    if (explicita) {
      // Entrada obsoleta ou com erro de digitacao nao pode aplicar uma escolha
      // que o pacote nunca ofereceu: a expressao registrada e conferida contra
      // o que o pacote declara hoje.
      if (explicita.expression !== c.licencaDeclarada) {
        pendentes.push(
          `${c.id}: a politica registra a expressao "${explicita.expression}" mas o pacote declara "${c.licencaDeclarada}"`,
        );
        continue;
      }
      if (typeof explicita.elected !== "string" || !explicita.elected.trim()) {
        pendentes.push(`${c.id}: entrada em licenseElections sem \`elected\` utilizavel`);
        continue;
      }
      const eleitas = licencasDaEleicao(explicita.elected);
      if (eleitas.erro) {
        pendentes.push(
          `${c.id}: a eleicao registrada "${explicita.elected}" nao e expressao SPDX valida (${eleitas.erro})`,
        );
        continue;
      }
      // Uma unica pergunta no lugar de tres: o conjunto eleito satisfaz a
      // expressao? Se satisfaz, nao cita licenca que ela nao ofereca e nao
      // deixa de fora nenhum termo obrigatorio — as duas coisas decorrem da
      // definicao de satisfacao, nao de uma conferencia a parte.
      if (!satisfaz(ast, eleitas.licencas)) {
        const obrigatorias = [...licencasObrigatorias(ast)];
        pendentes.push(
          `${c.id}: a eleicao registrada "${explicita.elected}" nao satisfaz "${c.licencaDeclarada}"` +
            (obrigatorias.length
              ? `; a expressao exige ${obrigatorias.join(", ")} em qualquer escolha`
              : ""),
        );
        continue;
      }
      const corr = corroboradas(eleitas.licencas, c.textos || []);
      if (!corr.ok) {
        pendentes.push(
          `${c.id}: eleicao registrada de ${explicita.elected} nao se sustenta — ${corr.motivo}`,
        );
        continue;
      }
      c.eleicao = { licenca: explicita.elected, origem: "registrada na politica" };
      continue;
    }

    // Elege-se automaticamente a primeira licenca da ordem de preferencia que
    // SOZINHA satisfaca a expressao e cujo texto esteja de fato reproduzido.
    // Exigir que ela sozinha satisfaca e o que impede eleger um termo de
    // conjuncao: em "MIT AND Zlib" nenhuma das duas basta, e o componente cai
    // corretamente na exigencia de eleicao explicita. Preferir um termo sem
    // texto produziria afirmacao falsa.
    const eleita = POLICY.licenseElectionPreference.find(
      (p) =>
        folhas.includes(p) &&
        satisfaz(ast, new Set([p])) &&
        corroboradas([p], c.textos || []).ok,
    );
    if (!eleita) {
      pendentes.push(
        `${c.id}: nenhuma licenca de "${c.licencaDeclarada}" satisfaz a expressao sozinha, consta da ordem de preferencia e tem o texto reproduzido no artefato; registre a eleicao em licenseElections`,
      );
      continue;
    }
    c.eleicao = { licenca: eleita, origem: "ordem de preferencia da politica" };
  }
  if (pendentes.length) {
    falhar(
      "Componentes distribuidos que oferecem escolha de licenca sem eleicao registrada:",
      pendentes,
    );
  }
}

// -------------------------------------------------------------------- cargo

function componentesCargo() {
  const args = [
    "metadata",
    "--locked",
    "--format-version",
    "1",
    "--filter-platform",
    POLICY.scope.cargo.targetTriple,
    "--manifest-path",
    resolve(RAIZ, POLICY.scope.cargo.manifest),
  ];
  const meta = JSON.parse(
    execFileSync("cargo", args, {
      cwd: RAIZ,
      encoding: "utf8",
      maxBuffer: 256 * 1024 * 1024,
    }),
  );

  const porId = new Map(meta.packages.map((p) => [p.id, p]));
  const nos = new Map((meta.resolve?.nodes || []).map((n) => [n.id, n]));
  const raizId = meta.resolve?.root;
  const kinds = new Set(POLICY.scope.cargo.includedDependencyKinds);

  const alcancados = new Set();
  const fila = [raizId];
  while (fila.length) {
    const id = fila.pop();
    if (!id || alcancados.has(id)) continue;
    alcancados.add(id);
    for (const d of nos.get(id)?.deps || []) {
      const normal = (d.dep_kinds || []).some((k) => kinds.has(k.kind ?? null));
      if (normal) fila.push(d.pkg);
    }
  }
  alcancados.delete(raizId);

  const base = join(
    process.env.CARGO_HOME || join(process.env.USERPROFILE || process.env.HOME || "", ".cargo"),
    "registry",
    "src",
  );
  const indices = existsSync(base) ? readdirSync(base) : [];

  const saida = [];
  for (const id of alcancados) {
    const p = porId.get(id);
    if (!p || !p.source) continue;
    let diretorio = null;
    for (const idx of indices) {
      const cand = join(base, idx, `${p.name}-${p.version}`);
      if (existsSync(cand)) {
        diretorio = cand;
        break;
      }
    }
    saida.push({
      ecossistema: "cargo",
      nome: p.name,
      versao: p.version,
      id: `${p.name}@${p.version}`,
      licencaDeclarada: p.license || null,
      origemPacote: p.source || null,
      diretorio,
    });
  }
  return saida.sort((a, b) => a.id.localeCompare(b.id));
}

// ------------------------------------------------------------------ montagem

const componentes = [...componentesNpm(), ...componentesCargo()];

// Texto vendorizado — de fallback ou de complemento — carrega proveniencia
// travada num commit especifico do upstream. Aplica-lo a um pacote que passou a
// vir de outra origem — git, `file:`, outro registro — publicaria a proveniencia
// de um artefato pelo de outro.
const vemDoRegistroCanonico = (ecossistema, origemPacote) =>
  ecossistema === "cargo"
    ? (origemPacote || "").startsWith(
        "registry+https://github.com/rust-lang/crates.io-index",
      )
    : (origemPacote || "").startsWith("https://registry.npmjs.org/");

const semTexto = [];
const naoDeclarados = [];
for (const c of componentes) {
  const fallback = POLICY.licenseFallbacks[c.id];
  if (fallback) {
    if (!vemDoRegistroCanonico(fallback.ecosystem, c.origemPacote)) {
      semTexto.push(
        `${c.id} (${c.ecossistema}): tem fallback declarado, mas o lockfile resolve para "${c.origemPacote ?? "origem ausente"}", que nao e o registro canonico; a proveniencia travada nao se aplica`,
      );
      continue;
    }
    const textos = fallback.fragments
      .map((f) => ({
        arquivo: POLICY.fragments[f].path,
        texto: (fragmentos.get(f) || "").trim(),
      }))
      .filter((t) => t.texto);
    // Fallback sem fragmento, ou apontando para arquivo so com espacos, nao
    // produz aviso nenhum: seguiria adiante emitindo cabecalho sem licenca.
    if (!textos.length) {
      semTexto.push(
        `${c.id} (${c.ecossistema}): o fallback declarado nao produz nenhum texto de licenca`,
      );
      continue;
    }
    c.origemDoTexto = "fragmento vendorizado";
    c.fallback = fallback;
    c.textos = textos;
    continue;
  }
  const achados = textoDeLicencaNoDiretorio(c.diretorio);
  if (achados) {
    c.origemDoTexto = "artefato baixado";
    c.textos = achados;
    // Complemento declarado: acrescenta o texto que a expressao exige e o
    // pacote nao reproduz, sem substituir o que ele proprio publica.
    const suplemento = POLICY.licenseSupplements?.[c.id];
    if (suplemento) {
      // Mesma amarra do fallback: o complemento tambem e texto vendorizado com
      // proveniencia travada, e nao vale para um artefato de outra origem.
      if (!vemDoRegistroCanonico(suplemento.ecosystem, c.origemPacote)) {
        semTexto.push(
          `${c.id} (${c.ecossistema}): tem complemento declarado, mas o lockfile resolve para "${c.origemPacote ?? "origem ausente"}", que nao e o registro canonico; a proveniencia travada nao se aplica`,
        );
        continue;
      }
      const extras = suplemento.fragments
        .map((f) => ({
          arquivo: POLICY.fragments[f].path,
          texto: (fragmentos.get(f) || "").trim(),
        }))
        .filter((t) => t.texto);
      if (!extras.length) {
        semTexto.push(
          `${c.id} (${c.ecossistema}): o complemento declarado nao produz nenhum texto`,
        );
        continue;
      }
      c.suplemento = suplemento;
      c.textos = [...c.textos, ...extras];
      c.origemDoTexto = "artefato baixado mais complemento declarado";
    }
    continue;
  }
  if (!c.diretorio) naoDeclarados.push(`${c.id} (${c.ecossistema}): artefato nao encontrado em disco`);
  else semTexto.push(`${c.id} (${c.ecossistema}): sem texto de licenca em ${c.diretorio}`);
}

if (semTexto.length || naoDeclarados.length) {
  falhar(
    "Componentes distribuidos sem texto de licenca. Registre cada um em scripts/legal/thirdparty-policy.mjs com origem imutavel e motivo, ou remova a dependencia:",
    [...naoDeclarados, ...semTexto],
  );
}

// Componente que declara UMA licenca so nunca passava por corroboracao: a
// conferencia por marcador existia apenas no caminho da eleicao. Um LICENSE
// nao-vazio contendo so o identificador SPDX, ou uma URL apontando para a
// licenca de verdade, era publicado como se fosse o texto integral. O gate
// passa a exigir que a licenca declarada esteja de fato reproduzida.
function corroborarLicencaUnica(componentes) {
  const pendentes = [];
  for (const c of componentes) {
    // Quem passou pela eleicao ja foi corroborado la.
    if (c.eleicao) continue;
    const declarada = (c.licencaDeclarada || "").trim();
    if (!declarada) continue;

    const inspecionada = POLICY.unverifiableLicenseDeclarations?.[c.id];
    if (inspecionada) {
      if (inspecionada.declared !== declarada) {
        pendentes.push(
          `${c.id} (${c.ecossistema}): a politica registra a declaracao "${inspecionada.declared}" mas o pacote declara "${declarada}"`,
        );
      }
      continue;
    }

    // Uma declaracao que nao analisa ja parou o gate em `elegerLicencas`, que
    // roda antes: chegar aqui sem inspecao registrada seria contradicao.
    const { ast, erro } = analisarExpressao(declarada);
    if (erro) continue;
    const folhas = folhasDaExpressao(ast);
    if (folhas.length !== 1) continue;

    // Sem marcador declarado nao se afirma nem se nega nada — e por isso o
    // componente para o gate, em vez de passar por omissao. Ou alguem declara
    // o marcador, ou registra a inspecao manual acima.
    const corr = corroboradas(folhas, c.textos || []);
    if (!corr.ok) {
      pendentes.push(
        corr.motivo.startsWith("sem marcador")
          ? `${c.id} (${c.ecossistema}): declara ${declarada}, para a qual a politica nao tem marcador; declare um em licenseTextMarkers ou registre a inspecao manual em unverifiableLicenseDeclarations`
          : `${c.id} (${c.ecossistema}): declara ${declarada} mas ${corr.motivo}`,
      );
    }
  }
  if (pendentes.length) {
    falhar(
      "Componentes cujo texto reproduzido nao sustenta a licenca declarada. Um arquivo que so aponta para a licenca nao e a licenca:",
      pendentes,
    );
  }
}

// A eleicao roda depois da coleta porque precisa do texto efetivamente
// reproduzido: so se elege licenca que acompanha o artefato.
elegerLicencas(componentes);
corroborarLicencaUnica(componentes);

// O cabecalho discrimina os componentes por procedencia do texto. Se um dia
// surgir uma quarta procedencia, a soma para de fechar e o leitor do arquivo
// nao teria como perceber: as parcelas simplesmente nao somariam o total. O
// gate reprova antes de emitir um cabecalho que nao se sustenta.
const PROCEDENCIAS = [
  "artefato baixado",
  "fragmento vendorizado",
  "artefato baixado mais complemento declarado",
];
const contarPorProcedencia = (p) =>
  componentes.filter((c) => c.origemDoTexto === p).length;
const somaDasProcedencias = PROCEDENCIAS.reduce(
  (total, p) => total + contarPorProcedencia(p),
  0,
);
if (somaDasProcedencias !== componentes.length) {
  const orfaos = componentes
    .filter((c) => !PROCEDENCIAS.includes(c.origemDoTexto))
    .map((c) => `${c.id} (${c.ecossistema}): procedencia "${c.origemDoTexto ?? "ausente"}"`);
  falhar(
    `As parcelas por procedencia somam ${somaDasProcedencias} e nao os ${componentes.length} componentes cobertos:`,
    orfaos,
  );
}

const barra = "=".repeat(78);
const linhas = [
  "AVISOS DE TERCEIROS - Maestro Editorial AI",
  "",
  `Este arquivo reproduz o texto de licenca de cada componente de terceiro incorporado`,
  `ao executavel distribuido. Ele acompanha o LICENSE, o NOTICE e o THIRDPARTY.md no`,
  `arquivo portatil.`,
  "",
  `Componentes cobertos: ${componentes.length}`,
  `  npm .....................: ${componentes.filter((c) => c.ecossistema === "npm").length}`,
  `  cargo ...................: ${componentes.filter((c) => c.ecossistema === "cargo").length}`,
  `  texto do proprio artefato: ${componentes.filter((c) => c.origemDoTexto === "artefato baixado").length}`,
  `  texto vendorizado .......: ${componentes.filter((c) => c.origemDoTexto === "fragmento vendorizado").length}`,
  `  artefato + complemento ..: ${componentes.filter((c) => c.origemDoTexto === "artefato baixado mais complemento declarado").length}`,
  "",
  ...(plataformaExcluidos.length
    ? [
        `Excluidos por restricao de plataforma (${POLICY.scope.npm.targetOs}/${POLICY.scope.npm.targetCpu}): ${plataformaExcluidos.length}`,
        `  ${plataformaExcluidos.join(", ")}`,
        "  Nao sao instalados nesta plataforma e portanto nao entram no artefato.",
        "",
      ]
    : []),
  `Dependencias de desenvolvimento nao constam: nao sao incorporadas ao executavel.`,
  `Gerado por scripts/generate-notices.mjs a partir de package-lock.json e de`,
  `cargo metadata --locked, filtrado para ${POLICY.scope.cargo.targetTriple}.`,
  `Codigo-fonte do produto: ${POLICY.project.sourceRepository}`,
  "",
];

// Dois artefatos distintos podem trazer as mesmas coordenadas. Quando isso
// acontece, o cabecalho de nome e versao deixa de distinguir um do outro, e o
// leitor do arquivo nao teria como saber a qual deles cada texto pertence: a
// origem passa a ser impressa junto, e so nesse caso.
const quantosArtefatosPorId = new Map();
for (const c of componentes) {
  const chave = `${c.ecossistema}|${c.id}`;
  quantosArtefatosPorId.set(chave, (quantosArtefatosPorId.get(chave) ?? 0) + 1);
}

for (const c of componentes) {
  linhas.push(barra, "");
  linhas.push(`${c.nome} ${c.versao}  (${c.ecossistema})`);
  if (quantosArtefatosPorId.get(`${c.ecossistema}|${c.id}`) > 1) {
    linhas.push(`Origem do artefato: ${c.origemPacote ?? "nao declarada no lockfile"}`);
  }
  if (c.licencaDeclarada) linhas.push(`Licenca declarada: ${c.licencaDeclarada}`);
  if (c.eleicao) {
    linhas.push(`Licenca eleita: ${c.eleicao.licenca} (${c.eleicao.origem})`);
  }
  if (c.fallback) {
    linhas.push(`Origem do texto: ${c.fallback.sourceRepository} @ ${c.fallback.revision}`);
    if (c.fallback.correspondingSource) {
      linhas.push(`Codigo-fonte correspondente: ${c.fallback.correspondingSource}`);
    }
    linhas.push(`Motivo do texto vendorizado: ${c.fallback.rationale}`);
  }
  if (c.suplemento) {
    linhas.push(
      `Origem do texto complementar: ${c.suplemento.sourceRepository} @ ${c.suplemento.revision}`,
    );
    linhas.push(`Motivo do complemento: ${c.suplemento.rationale}`);
  }
  linhas.push("");
  for (const t of c.textos) {
    linhas.push(`--- ${t.arquivo} ---`, "", t.texto, "");
  }
}
linhas.push(barra, "");

const conteudo = `${linhas.join("\n").trimEnd()}\n`;

if (MODO_CHECK) {
  if (!existsSync(SAIDA)) {
    falhar(`${POLICY.outputs.notices} nao existe.`, ["Rode: npm run notices"]);
  }
  const atual = paraLf(readFileSync(SAIDA, "utf8"));
  if (atual !== conteudo) {
    falhar(`${POLICY.outputs.notices} esta desatualizado em relacao as dependencias.`, [
      `commitado: ${atual.length} chars, sha256 ${sha256(atual)}`,
      `esperado : ${conteudo.length} chars, sha256 ${sha256(conteudo)}`,
      "Rode: npm run notices",
    ]);
  }
  console.log(
    `${POLICY.outputs.notices} confere: ${componentes.length} componentes, sha256 ${sha256(conteudo)}`,
  );
} else {
  writeFileSync(SAIDA, conteudo, "utf8");
  console.log(
    `${POLICY.outputs.notices} gravado: ${componentes.length} componentes, ${conteudo.length} chars, sha256 ${sha256(conteudo)}`,
  );
}
