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
    if (vistos.has(id)) continue;
    vistos.add(id);
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
// as obrigacoes assumidas. Somente duas formas sao eleitas automaticamente,
// ambas inequivocas: uma disjuncao plana e a forma legada do Cargo. Qualquer
// outra e recusada e exige entrada explicita. Nao se interpreta aqui a
// gramatica do SPDX.
// Um identificador SPDX e um token curto de letras, digitos, ponto, mais e
// hifen. Nunca contem dois-pontos nem barra. Exigir essa forma impede que uma
// URL de licenca — que o campo `license` de pacotes antigos as vezes traz —
// seja lida como se fosse uma escolha entre alternativas.
const IDENTIFICADOR_SPDX = /^[A-Za-z0-9][A-Za-z0-9.+-]*$/u;

const pareceUrl = (expressao) => expressao.includes(":");

function termosDeEscolha(expressao) {
  const e = expressao.trim();
  if (pareceUrl(e)) return null;
  if (e.includes("(") || e.includes(")")) return null;
  if (/\bAND\b/u.test(e) || /\bWITH\b/u.test(e)) return null;
  if (/\bOR\b/u.test(e)) {
    const termos = e
      .split(/\bOR\b/u)
      .map((t) => t.trim())
      .filter(Boolean);
    if (termos.length >= 2 && termos.every((t) => IDENTIFICADOR_SPDX.test(t))) {
      return termos;
    }
    return null;
  }
  // Forma legada do Cargo. Aparece com e sem espacos ao redor da barra
  // (`MIT/Apache-2.0` e `Apache-2.0 / MIT`), e ambas sao a mesma disjuncao.
  if (e.includes("/")) {
    const termos = e
      .split("/")
      .map((t) => t.trim())
      .filter(Boolean);
    if (termos.length >= 2 && termos.every((t) => IDENTIFICADOR_SPDX.test(t))) {
      return termos;
    }
  }
  return null;
}

// Toda expressao composta precisa passar pela validacao, nao so as de escolha.
// `MIT AND Apache-2.0` e `Apache-2.0 WITH LLVM-exception` nao oferecem opcao,
// mas exigem que MAIS DE UM texto acompanhe o artefato — e um unico LICENSE
// legivel nao prova isso. Elas caem em `termosDeEscolha` como forma nao
// trivial e passam a exigir entrada explicita.
function precisaDeValidacao(expressao) {
  if (!expressao) return false;
  // Uma URL nao oferece escolha nenhuma: a barra ali e caminho, nao disjuncao.
  if (pareceUrl(expressao)) return false;
  return (
    /\bOR\b/u.test(expressao) ||
    /\bAND\b/u.test(expressao) ||
    /\bWITH\b/u.test(expressao) ||
    expressao.includes("/")
  );
}

// A licenca eleita precisa estar efetivamente reproduzida no artefato. Sem
// isso, o arquivo pode afirmar Apache-2.0 enquanto reproduz o texto da CC0.
// O texto das licencas vem quebrado em larguras diferentes conforme o pacote:
// o LICENSE-MIT do unicode-ident quebra "this permission notice / shall be
// included" no meio da frase. Comparar trecho literal contra isso falha por
// motivo tipografico, nao juridico. Normaliza-se espaco em branco dos dois
// lados antes de comparar.
const normalizarEspacos = (t) => t.replace(/\s+/gu, " ").trim();

function corroborada(licenca, textos) {
  const corpo = normalizarEspacos(textos.map((t) => t.texto).join("\n"));
  const conjuntos = licenca
    .split(/\bAND\b/u)
    .map((t) => t.trim())
    .filter(Boolean);
  for (const termo of conjuntos) {
    const marcadores = POLICY.licenseTextMarkers[termo];
    if (!marcadores) return { ok: false, motivo: `sem marcador declarado para ${termo}` };
    if (!marcadores.some((m) => corpo.includes(normalizarEspacos(m)))) {
      return {
        ok: false,
        motivo: `nenhum marcador de ${termo} aparece no texto reproduzido`,
      };
    }
  }
  return { ok: true };
}

function elegerLicencas(componentes) {
  const pendentes = [];
  for (const c of componentes) {
    const expressao = c.licencaDeclarada;
    if (!precisaDeValidacao(expressao)) continue;

    const explicita = POLICY.licenseElections[c.id];
    if (explicita) {
      // Entrada obsoleta ou com erro de digitacao nao pode aplicar uma escolha
      // que o pacote nunca ofereceu: a expressao registrada e conferida contra
      // o que o pacote declara hoje.
      if (explicita.expression !== expressao) {
        pendentes.push(
          `${c.id}: a politica registra a expressao "${explicita.expression}" mas o pacote declara "${expressao}"`,
        );
        continue;
      }
      // Objeto vazio ou sem `elected` produziria `Licenca eleita: undefined`.
      if (typeof explicita.elected !== "string" || !explicita.elected.trim()) {
        pendentes.push(
          `${c.id}: entrada em licenseElections sem \`elected\` utilizavel`,
        );
        continue;
      }
      // A eleita precisa ser permitida pela propria expressao: registrar GPL
      // para "MIT OR Apache-2.0" publicaria uma escolha que o pacote nao
      // oferece. Termos de conjuncao (`A AND B`) sao conferidos um a um.
      const oferecidos = expressao
        .split(/\bOR\b|\bAND\b|\//u)
        .map((t) => t.replace(/[()]/gu, "").trim())
        .filter(Boolean);
      const eleitos = explicita.elected
        .split(/\bAND\b/u)
        .map((t) => t.trim())
        .filter(Boolean);
      const forasteiros = eleitos.filter((t) => !oferecidos.includes(t));
      if (forasteiros.length) {
        pendentes.push(
          `${c.id}: a eleicao registrada cita ${forasteiros.join(", ")}, que a expressao "${expressao}" nao oferece`,
        );
        continue;
      }
      const corr = corroborada(explicita.elected, c.textos || []);
      if (!corr.ok) {
        pendentes.push(
          `${c.id}: eleicao registrada de ${explicita.elected} nao se sustenta — ${corr.motivo}`,
        );
        continue;
      }
      c.eleicao = { licenca: explicita.elected, origem: "registrada na politica" };
      continue;
    }

    const termos = termosDeEscolha(expressao);
    if (!termos) {
      pendentes.push(
        `${c.id}: expressao "${expressao}" nao e uma escolha trivial e precisa de entrada em licenseElections`,
      );
      continue;
    }
    // Elege-se o primeiro termo que a preferencia indique E cujo texto esteja
    // de fato reproduzido. Preferir um termo sem texto produziria afirmacao
    // falsa; se nenhum se sustentar, o gate reprova e pede decisao explicita.
    const candidatos = POLICY.licenseElectionPreference.filter((p) =>
      termos.includes(p),
    );
    if (!candidatos.length) {
      pendentes.push(
        `${c.id}: nenhum termo de "${expressao}" consta da ordem de preferencia; registre a eleicao em licenseElections`,
      );
      continue;
    }
    const eleita = candidatos.find((p) => corroborada(p, c.textos || []).ok);
    if (!eleita) {
      pendentes.push(
        `${c.id}: nenhum termo de "${expressao}" tem o texto reproduzido no artefato; registre a eleicao em licenseElections`,
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

const semTexto = [];
const naoDeclarados = [];
for (const c of componentes) {
  const fallback = POLICY.licenseFallbacks[c.id];
  if (fallback) {
    // O fallback carrega proveniencia travada num commit especifico. Aplica-lo
    // a um pacote que passou a vir de outra origem — git, `file:`, outro
    // registro — publicaria a proveniencia de um artefato pelo de outro.
    const registroCanonico =
      fallback.ecosystem === "cargo"
        ? (c.origemPacote || "").startsWith("registry+https://github.com/rust-lang/crates.io-index")
        : (c.origemPacote || "").startsWith("https://registry.npmjs.org/");
    if (!registroCanonico) {
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

// A eleicao roda depois da coleta porque precisa do texto efetivamente
// reproduzido: so se elege licenca que acompanha o artefato.
elegerLicencas(componentes);

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
  "",
  ...(plataformaExcluidos.length
    ? [
        `Excluidos por restricao de plataforma (${process.platform}/${process.arch}): ${plataformaExcluidos.length}`,
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

for (const c of componentes) {
  linhas.push(barra, "");
  linhas.push(`${c.nome} ${c.versao}  (${c.ecossistema})`);
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
