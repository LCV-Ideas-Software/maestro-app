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
  const achados = entradas.filter((e) => {
    const minusculo = e.toLowerCase();
    if (!POLICY.licenseFilePrefixes.some((p) => minusculo.startsWith(p))) return false;
    return !POLICY.licenseFileIgnoredExtensions.some((ext) => minusculo.endsWith(ext));
  });
  if (!achados.length) return null;
  const partes = [];
  for (const a of achados.sort()) {
    // Le direto em vez de checar o tipo antes: consultar e depois usar deixa
    // uma janela entre as duas chamadas. Um diretorio faz readFileSync lancar
    // EISDIR, que o catch trata, com o mesmo efeito e sem a janela.
    try {
      const t = paraLf(readFileSync(join(dir, a), "utf8")).trim();
      if (t) partes.push({ arquivo: a, texto: t });
    } catch {
      /* nao e arquivo legivel: segue */
    }
  }
  return partes.length ? partes : null;
}

function componentesNpm() {
  const lock = JSON.parse(readFileSync(resolve(RAIZ, "package-lock.json"), "utf8"));
  const marcador = POLICY.scope.npm.excludeDevMarker;
  const saida = [];
  const vistos = new Set();
  for (const [chave, meta] of Object.entries(lock.packages || {})) {
    if (!chave.startsWith("node_modules/")) continue;
    if (meta[marcador] === true) continue;
    const nome = meta.name || nomeDaChaveDoLock(chave);
    const versao = meta.version;
    if (!versao) continue;
    const id = `${nome}@${versao}`;
    if (vistos.has(id)) continue;
    vistos.add(id);
    saida.push({
      ecossistema: "npm",
      nome,
      versao,
      id,
      licencaDeclarada: meta.license || null,
      diretorio: acharDiretorioNpm(chave, nome, versao),
    });
  }
  return saida.sort((a, b) => a.id.localeCompare(b.id));
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
    c.origemDoTexto = "fragmento vendorizado";
    c.fallback = fallback;
    c.textos = fallback.fragments.map((f) => ({
      arquivo: POLICY.fragments[f].path,
      texto: fragmentos.get(f).trim(),
    }));
    continue;
  }
  const achados = textoDeLicencaNoDiretorio(c.diretorio);
  if (achados) {
    c.origemDoTexto = "artefato baixado";
    c.textos = achados;
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
