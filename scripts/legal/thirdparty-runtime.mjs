import { createHash } from "node:crypto";
import { dirname, isAbsolute, relative, sep } from "node:path";

import npmInstallChecks from "npm-install-checks";
import spdxParse from "spdx-expression-parse";

export function resolverMetaNpm(packages, chave, metaOriginal) {
  if (metaOriginal.link !== true) {
    return {
      meta: metaOriginal,
      origemDaIdentidade: metaOriginal.resolved || null,
    };
  }

  const alvo = metaOriginal.resolved ? packages[metaOriginal.resolved] : null;
  if (!alvo) {
    return {
      erro: `${chave}: entrada com link nao resolvida (resolved=${metaOriginal.resolved ?? "ausente"})`,
    };
  }

  return {
    meta: { ...alvo, name: alvo.name || metaOriginal.name },
    origemDaIdentidade: metaOriginal.resolved,
  };
}

export function plataformaExcluida(meta, alvo) {
  try {
    npmInstallChecks.checkPlatform(meta, false, {
      os: alvo.targetOs,
      cpu: alvo.targetCpu,
      libc: alvo.targetLibc,
    });
    return false;
  } catch (erro) {
    if (erro?.code === "EBADPLATFORM") return true;
    throw erro;
  }
}

export function validarVinculoDoArtefato(registro, componente) {
  if (
    typeof registro?.ecosystem !== "string" ||
    typeof registro?.source !== "string"
  ) {
    return { ok: false, tipo: "politica-incompleta" };
  }
  if (registro.ecosystem !== componente?.ecossistema) {
    return {
      ok: false,
      tipo: "ecossistema-divergente",
      esperado: registro.ecosystem,
      encontrado: componente?.ecossistema ?? null,
    };
  }
  if (registro.source !== componente?.origemPacote) {
    return {
      ok: false,
      tipo: "origem-divergente",
      esperado: registro.source,
      encontrado: componente?.origemPacote ?? null,
    };
  }
  return { ok: true };
}

export function selecionarRegistroDoArtefato(entrada, componente) {
  const registros = Array.isArray(entrada)
    ? entrada
    : entrada && typeof entrada === "object"
      ? [entrada]
      : [];
  if (!registros.length) {
    return { ok: false, tipo: "politica-incompleta" };
  }

  const incompletos = registros.filter(
    (registro) =>
      validarVinculoDoArtefato(registro, componente).tipo ===
      "politica-incompleta",
  );
  if (incompletos.length) {
    return { ok: false, tipo: "politica-incompleta" };
  }

  const correspondentes = registros.filter(
    (registro) => validarVinculoDoArtefato(registro, componente).ok,
  );
  if (correspondentes.length > 1) {
    return {
      ok: false,
      tipo: "politica-duplicada",
      quantidade: correspondentes.length,
    };
  }
  if (correspondentes.length === 1) {
    return { ok: true, registro: correspondentes[0] };
  }

  const mesmoEcossistema = registros.filter(
    (registro) => registro.ecosystem === componente?.ecossistema,
  );
  if (!mesmoEcossistema.length) {
    return {
      ok: false,
      tipo: "ecossistema-divergente",
      esperados: [...new Set(registros.map((registro) => registro.ecosystem))],
      encontrado: componente?.ecossistema ?? null,
    };
  }
  return {
    ok: false,
    tipo: "origem-divergente",
    esperadas: [
      ...new Set(mesmoEcossistema.map((registro) => registro.source)),
    ],
    encontrada: componente?.origemPacote ?? null,
  };
}

const SHA256_HEX = /^[0-9a-f]{64}$/u;

export const sha256TextoDeLicenca = (texto) =>
  createHash("sha256")
    .update(texto.replace(/\r\n?/gu, "\n").trim(), "utf8")
    .digest("hex");

// A leitura manual nao pode sobreviver a uma troca silenciosa do material que
// foi inspecionado. A origem seleciona o artefato; esta prova fixa o conjunto
// exato de textos efetivamente reproduzidos, depois da mesma normalizacao de
// fim de linha e bordas usada pela coleta do gerador.
export function validarEvidenciaTextual(inspecao, textos) {
  const evidencias = inspecao?.textEvidence;
  if (!Array.isArray(evidencias) || !evidencias.length) {
    return {
      ok: false,
      tipo: "politica-incompleta",
      motivo: "textEvidence precisa declarar ao menos um arquivo e seu SHA-256",
    };
  }

  const esperados = new Map();
  for (const evidencia of evidencias) {
    if (
      typeof evidencia?.file !== "string" ||
      !evidencia.file.trim() ||
      typeof evidencia.sha256 !== "string" ||
      !SHA256_HEX.test(evidencia.sha256)
    ) {
      return {
        ok: false,
        tipo: "politica-incompleta",
        motivo:
          "cada item de textEvidence precisa registrar file e SHA-256 lowercase completo",
      };
    }
    if (esperados.has(evidencia.file)) {
      return {
        ok: false,
        tipo: "arquivo-duplicado",
        motivo: `textEvidence repete o arquivo ${evidencia.file}`,
      };
    }
    esperados.set(evidencia.file, evidencia.sha256);
  }

  if (!Array.isArray(textos)) {
    return {
      ok: false,
      tipo: "conjunto-divergente",
      motivo: "o artefato nao forneceu um conjunto de textos para conferir",
    };
  }

  const encontrados = new Map();
  for (const texto of textos) {
    if (
      typeof texto?.arquivo !== "string" ||
      !texto.arquivo.trim() ||
      typeof texto.texto !== "string"
    ) {
      return {
        ok: false,
        tipo: "texto-invalido",
        motivo: "cada texto coletado precisa registrar arquivo e conteudo textual",
      };
    }
    if (encontrados.has(texto.arquivo)) {
      return {
        ok: false,
        tipo: "arquivo-duplicado",
        motivo: `o conjunto coletado repete o arquivo ${texto.arquivo}`,
      };
    }
    encontrados.set(texto.arquivo, sha256TextoDeLicenca(texto.texto));
  }

  const faltantes = [...esperados.keys()].filter(
    (arquivo) => !encontrados.has(arquivo),
  );
  const inesperados = [...encontrados.keys()].filter(
    (arquivo) => !esperados.has(arquivo),
  );
  if (faltantes.length || inesperados.length) {
    return {
      ok: false,
      tipo: "conjunto-divergente",
      faltantes: faltantes.sort(),
      inesperados: inesperados.sort(),
      motivo: `o conjunto de textos diverge da inspecao (faltantes: ${faltantes.join(", ") || "nenhum"}; inesperados: ${inesperados.join(", ") || "nenhum"})`,
    };
  }

  const divergentes = [...esperados].flatMap(([arquivo, sha256]) =>
    encontrados.get(arquivo) === sha256 ? [] : [arquivo],
  );
  if (divergentes.length) {
    return {
      ok: false,
      tipo: "texto-divergente",
      arquivos: divergentes.sort(),
      motivo: `o SHA-256 do texto diverge da inspecao para ${divergentes.join(", ")}`,
    };
  }

  return { ok: true };
}

const normalizarBarraLegada = (expressao) =>
  expressao.includes("/")
    ? expressao
        .split("/")
        .map((termo) => termo.trim())
        .join(" OR ")
    : expressao;

export function analisarExpressao(expressao) {
  try {
    return { ast: spdxParse(normalizarBarraLegada(expressao.trim())) };
  } catch (erro) {
    return { erro: erro.message };
  }
}

export const folhaComoTexto = (no) =>
  `${no.license}${no.plus ? "+" : ""}${no.exception ? ` WITH ${no.exception}` : ""}`;

export function folhasDaExpressao(no) {
  if (no.license) return [folhaComoTexto(no)];
  return [...folhasDaExpressao(no.left), ...folhasDaExpressao(no.right)];
}

export function licencasObrigatorias(no) {
  if (no.license) return new Set([folhaComoTexto(no)]);
  const esquerda = licencasObrigatorias(no.left);
  const direita = licencasObrigatorias(no.right);
  return no.conjunction === "and"
    ? new Set([...esquerda, ...direita])
    : new Set([...esquerda].filter((licenca) => direita.has(licenca)));
}

export function satisfaz(no, escolhidas) {
  if (no.license) return escolhidas.has(folhaComoTexto(no));
  return no.conjunction === "and"
    ? satisfaz(no.left, escolhidas) && satisfaz(no.right, escolhidas)
    : satisfaz(no.left, escolhidas) || satisfaz(no.right, escolhidas);
}

export function licencasDaEleicao(eleita) {
  const { ast, erro } = analisarExpressao(eleita);
  return erro ? { erro } : { ast, licencas: new Set(folhasDaExpressao(ast)) };
}

const contemDisjuncao = (no) =>
  !no.license &&
  (no.conjunction === "or" ||
    contemDisjuncao(no.left) ||
    contemDisjuncao(no.right));

export function validarEleicao(declarada, eleita) {
  const declaracao = analisarExpressao(declarada);
  if (declaracao.erro) {
    return { ok: false, tipo: "declaracao-invalida", erro: declaracao.erro };
  }

  const escolha = licencasDaEleicao(eleita);
  if (escolha.erro) {
    return { ok: false, tipo: "eleicao-invalida", erro: escolha.erro };
  }
  if (contemDisjuncao(escolha.ast)) {
    return { ok: false, tipo: "eleicao-ambigua" };
  }

  const folhas = folhasDaExpressao(declaracao.ast);
  const oferecidas = new Set(folhas);
  const forasteiras = [...escolha.licencas].filter(
    (licenca) => !oferecidas.has(licenca),
  );
  if (forasteiras.length) {
    return { ok: false, tipo: "forasteiras", forasteiras };
  }

  if (!satisfaz(declaracao.ast, escolha.licencas)) {
    return {
      ok: false,
      tipo: "nao-satisfaz",
      obrigatorias: [...licencasObrigatorias(declaracao.ast)],
    };
  }

  return {
    ok: true,
    ast: declaracao.ast,
    folhas,
    licencas: escolha.licencas,
  };
}

export const normalizarParaComparar = (texto) =>
  texto
    .split("\n")
    .map((linha) => linha.replace(/^\s*(?:\/\/+|#+|\*+)\s?/u, ""))
    .join("\n")
    .replace(/\s+/gu, " ")
    .trim()
    .toLowerCase();

export function corroboradas(licencas, textos, marcadoresPorLicenca) {
  const corpo = normalizarParaComparar(
    textos.map((texto) => texto.texto).join("\n"),
  );
  for (const termo of licencas) {
    const marcadores = marcadoresPorLicenca[termo];
    if (!marcadores) {
      return { ok: false, motivo: `sem marcador declarado para ${termo}` };
    }
    if (
      !marcadores.some((marcador) =>
        corpo.includes(normalizarParaComparar(marcador)),
      )
    ) {
      return {
        ok: false,
        motivo: `nenhum marcador de ${termo} aparece no texto reproduzido`,
      };
    }
  }
  return { ok: true };
}

const dentroDoRepositorio = (raiz, caminho) => {
  const rel = relative(raiz, caminho);
  return (
    rel !== "" &&
    !isAbsolute(rel) &&
    rel !== ".." &&
    !rel.startsWith(`..${sep}`)
  );
};

export function componentesCargoDaMetadata(
  metadata,
  { includedDependencyKinds, repositoryRoot },
) {
  const erros = [];
  const porId = new Map(
    (metadata.packages || []).map((pacote) => [pacote.id, pacote]),
  );
  const nos = new Map((metadata.resolve?.nodes || []).map((no) => [no.id, no]));
  const raizId = metadata.resolve?.root;
  const kinds = new Set(includedDependencyKinds);

  if (!raizId || !porId.has(raizId) || !nos.has(raizId)) {
    return {
      componentes: [],
      erros: ["cargo metadata nao informou uma raiz resolvida completa"],
    };
  }

  const alcancados = new Set();
  const fila = [raizId];
  while (fila.length) {
    const id = fila.pop();
    if (!id || alcancados.has(id)) continue;
    alcancados.add(id);
    const no = nos.get(id);
    if (!no) {
      erros.push(`${id}: ausente de resolve.nodes`);
      continue;
    }
    for (const dependencia of no.deps || []) {
      const normal = (dependencia.dep_kinds || []).some((kind) =>
        kinds.has(kind.kind ?? null),
      );
      if (normal) fila.push(dependencia.pkg);
    }
  }
  alcancados.delete(raizId);

  const componentes = [];
  for (const id of alcancados) {
    const pacote = porId.get(id);
    if (!pacote) {
      erros.push(`${id}: ausente de packages`);
      continue;
    }
    if (!pacote.manifest_path || !isAbsolute(pacote.manifest_path)) {
      erros.push(`${id}: manifest_path absoluto ausente em cargo metadata`);
      continue;
    }

    const diretorio = dirname(pacote.manifest_path);
    let origemPacote = pacote.source || null;
    if (!origemPacote) {
      if (!dentroDoRepositorio(repositoryRoot, diretorio)) {
        erros.push(
          `${id}: dependencia path fora do repositorio (${pacote.manifest_path})`,
        );
        continue;
      }
      origemPacote = `path:${relative(repositoryRoot, diretorio).split(sep).join("/")}`;
    }

    componentes.push({
      ecossistema: "cargo",
      nome: pacote.name,
      versao: pacote.version,
      id: `${pacote.name}@${pacote.version}`,
      licencaDeclarada: pacote.license || null,
      origemPacote,
      diretorio,
    });
  }

  return {
    componentes: componentes.sort((a, b) => a.id.localeCompare(b.id)),
    erros,
  };
}
