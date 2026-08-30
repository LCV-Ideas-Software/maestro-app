// Politica de avisos de terceiros do artefato distribuido.
//
// Este arquivo declara, de forma congelada e auditavel, os textos de licenca
// que precisam acompanhar o executavel e nao podem ser extraidos do artefato
// baixado, porque o publicador nao os inclui. Cada entrada registra o motivo,
// a origem imutavel e a revisao exata de onde o texto veio.
//
// A forma segue o padrao ja usado na frota em astrologo-app/scripts/legal.
//
// Registro tecnico de compliance; nao constitui parecer juridico.

export const POLICY = Object.freeze({
  project: Object.freeze({
    sourceRepository: "https://github.com/LCV-Ideas-Software/maestro-app",
    license: "AGPL-3.0-or-later",
  }),

  // Como o universo de componentes distribuidos e determinado. Sao as mesmas
  // fontes oficiais que o gate de inventario ja consome; dependencia de
  // desenvolvimento nao embarca no executavel e nao gera obrigacao de aviso.
  scope: Object.freeze({
    npm: Object.freeze({
      lock: "package-lock.json",
      // O npm marca com `dev: true` toda entrada alcancavel apenas por
      // devDependencies. Sao excluidas.
      excludeDevMarker: "dev",
    }),
    cargo: Object.freeze({
      manifest: "src-tauri/Cargo.toml",
      // Plataforma do artefato publicado. Sem o filtro, o grafo traz crates de
      // outros alvos que nao entram neste binario.
      targetTriple: "x86_64-pc-windows-msvc",
      // Somente arestas de dependencia normal. build-dependencies rodam na
      // maquina de build e nao sao ligadas ao executavel distribuido.
      includedDependencyKinds: Object.freeze([null]),
    }),
  }),

  outputs: Object.freeze({
    notices: "THIRD-PARTY-NOTICES.txt",
  }),

  // Como o texto de licenca e localizado dentro do artefato baixado.
  //
  // Casar por nome exato nao funciona: os publicadores usam formas muito
  // diferentes para o mesmo arquivo. Observado nas dependencias deste projeto,
  // em 30/08/2026: LICENSE_MIT e LICENSE_APACHE-2.0 (tauri, @tauri-apps/api),
  // LICENSE.markdown (jszip), LICENSE-MIT.txt (punycode.js, iri-string),
  // LICENSE-APACHE.md e LICENSE-ZLIB.md (tinyvec, raw-window-handle),
  // LICENSE.md (@tiptap). O criterio e o prefixo do nome, sem diferenciar
  // maiusculas, aceitando qualquer sufixo e extensao.
  // Arquivos que CARREGAM o texto da licenca. Pelo menos um deles, ou um
  // fallback declarado, e obrigatorio para cada componente distribuido.
  licenseFilePrefixes: Object.freeze([
    "license",
    "licence",
    "copying",
    "unlicense",
  ]),

  // Arquivos SUPLEMENTARES. Sao incluidos nos avisos quando existem, mas nunca
  // satisfazem sozinhos a exigencia acima: um NOTICE da Apache-2.0 e material
  // adicional exigido pela clausula 4(d), nao o texto da licenca. Aceitar um
  // NOTICE isolado como suficiente deixaria passar um componente sem licenca.
  supplementalFilePrefixes: Object.freeze(["notice"]),

  // Extensoes que nao carregam o texto da licenca e portanto nao contam como
  // aviso, mesmo quando o nome comeca com um dos prefixos acima.
  licenseFileIgnoredExtensions: Object.freeze([".spdx", ".json", ".xml"]),

  // Eleicao de licenca em expressoes de escolha.
  //
  // Quando um componente oferece mais de uma licenca, e a eleicao que determina
  // as obrigacoes assumidas. Sem registro, nada impede que uma dependencia nova
  // passe a oferecer escolha sem que ninguem decida qual foi tomada.
  //
  // A eleicao automatica so vale para as duas formas inequivocas: uma disjuncao
  // plana (`A OR B OR C`) e a forma legada do Cargo (`A/B`, com ou sem espacos
  // ao redor da barra). Qualquer outra expressao — com parenteses, com AND, com
  // WITH — precisa de entrada em `licenseElections`, senao o gate reprova. Nao
  // ha aqui um parser de SPDX escrito a mao: formas nao triviais nao sao
  // interpretadas, sao recusadas.
  licenseElectionPreference: Object.freeze([
    "MIT",
    "ISC",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "Apache-2.0",
    "0BSD",
    "MIT-0",
    "Unlicense",
    "Zlib",
    "BSL-1.0",
    "CC0-1.0",
  ]),

  // Corroboracao da eleicao pelo texto efetivamente reproduzido.
  //
  // Eleger uma licenca cujo texto nao acompanha o artefato produz uma afirmacao
  // falsa: o arquivo diria que Apache-2.0 foi eleita enquanto reproduz o texto
  // da CC0. Cada identificador elegivel declara aqui um trecho caracteristico
  // do seu proprio texto, e a eleicao so vale se ao menos um deles aparecer no
  // que foi reproduzido.
  //
  // Isto nao e deteccao de licenca: e uma tabela declarada e auditavel. Um
  // identificador sem marcador nao pode ser eleito, e o gate diz isso em vez de
  // aceitar em silencio.
  licenseTextMarkers: Object.freeze({
    MIT: Object.freeze([
      "Permission is hereby granted, free of charge",
    ]),
    "MIT-0": Object.freeze([
      "Permission is hereby granted, free of charge",
    ]),
    // Precisa ser frase que so existe no CORPO da licenca. "Apache License" e
    // a URL aparecem tambem em arquivos que apenas APONTAM para a licenca sem
    // reproduzi-la, e aceitar isso faria o gate corroborar um ponteiro.
    "Apache-2.0": Object.freeze([
      "TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION",
    ]),
    "BSD-2-Clause": Object.freeze([
      "Redistributions of source code must retain the above copyright notice",
    ]),
    "BSD-3-Clause": Object.freeze([
      "Neither the name of",
    ]),
    ISC: Object.freeze([
      "Permission to use, copy, modify, and/or distribute this software",
    ]),
    "0BSD": Object.freeze([
      "Permission to use, copy, modify, and/or distribute this software",
    ]),
    "CC0-1.0": Object.freeze([
      "CC0 1.0 Universal",
      "Creative Commons Legal Code",
    ]),
    "Unicode-3.0": Object.freeze([
      "UNICODE LICENSE",
      "Unicode",
    ]),
    Unlicense: Object.freeze([
      "This is free and unencumbered software released into the public domain",
    ]),
    Zlib: Object.freeze([
      "This software is provided 'as-is'",
      "altered source versions must be plainly marked",
    ]),
    "MPL-2.0": Object.freeze([
      "Mozilla Public License",
    ]),
    "BSL-1.0": Object.freeze([
      "Boost Software License",
    ]),
  }),

  // Eleicoes explicitas, por `<nome>@<versao>`. Necessarias para toda expressao
  // que nao seja uma das duas formas triviais. Fixar a versao impede que a
  // decisao sobreviva em silencio a uma atualizacao de dependencia.
  //
  // `expression` e conferida contra o que o pacote declara: entrada obsoleta ou
  // com erro de digitacao reprova em vez de aplicar uma escolha que o pacote
  // nunca ofereceu.
  licenseElections: Object.freeze({
    "siphasher@1.0.2": Object.freeze({
      expression: "MIT/Apache-2.0",
      elected: "MIT",
      rationale:
        "O crate nao reproduz nenhuma das duas licencas, so um ponteiro para elas, e o upstream tambem nao. Elege-se MIT, primeira da ordem de preferencia entre as oferecidas, com o texto vendorizado em scripts/legal/siphasher-mit.txt.",
    }),
    "dunce@1.0.5": Object.freeze({
      expression: "CC0-1.0 OR MIT-0 OR Apache-2.0",
      elected: "CC0-1.0",
      rationale:
        "A ordem de preferencia elegeria Apache-2.0, mas o crate empacota um unico LICENSE, e o texto nele e o da CC0-1.0. Eleger uma licenca cujo texto nao acompanha o artefato produziria afirmacao falsa; elege-se a que esta efetivamente reproduzida.",
    }),
    "dompurify@3.4.14": Object.freeze({
      expression: "(MPL-2.0 OR Apache-2.0)",
      elected: "Apache-2.0",
      rationale:
        "A expressao vem entre parenteses e portanto nao e eleita automaticamente. Elege-se Apache-2.0: e permissiva e evita as obrigacoes de arquivo da MPL-2.0 sobre um componente que e embutido no bundle distribuido.",
    }),
    "jszip@3.10.1": Object.freeze({
      expression: "(MIT OR GPL-3.0-or-later)",
      elected: "MIT",
      rationale:
        "A expressao vem entre parenteses e portanto nao e eleita automaticamente. Elege-se MIT: a alternativa e copyleft forte e incompativel com a distribuicao de um executavel proprietario de terceiros embutindo o componente.",
    }),
    "unicode-ident@1.0.24": Object.freeze({
      expression: "(MIT OR Apache-2.0) AND Unicode-3.0",
      elected: "MIT AND Unicode-3.0",
      rationale:
        "Expressao conjuntiva: a escolha entre MIT e Apache-2.0 e livre, mas a Unicode-3.0 aplica-se simultaneamente e nao e opcional. Elege-se MIT para o termo disjuntivo; os avisos da Unicode-3.0 continuam exigidos junto.",
    }),
  }),

  // Textos vendorizados. O sha256 e do arquivo inteiro, cabecalho de
  // proveniencia incluido, e e conferido em tempo de execucao: se alguem
  // editar um fragmento sem atualizar a politica, o gate reprova.
  fragments: Object.freeze({
    webview2Mit: Object.freeze({
      path: "scripts/legal/webview2-rs-mit.txt",
      sha256: "5c75e58acce81f033b54f2363e5c668f3414c7ffaf5d3cff46160ef7c3832643",
    }),
    unicMit: Object.freeze({
      path: "scripts/legal/rust-unic-mit.txt",
      sha256: "f0fbf20f1157a0f31b7d5dfee2db990e63555757b6256bcf976cb960dd08fc56",
    }),
    unicApache: Object.freeze({
      path: "scripts/legal/rust-unic-apache-2.0.txt",
      sha256: "d951cc6bb77d0d6c0dd26edc8f1e84d9168080bdb3e8ede75f8782495f48354f",
    }),
    selectorsMpl: Object.freeze({
      path: "scripts/legal/selectors-mpl-2.0.txt",
      sha256: "23018b646001457ad9dc56311bd3d63cdf23d8e4a3e7822419ab5dc532b7a043",
    }),
    siphasherMit: Object.freeze({
      path: "scripts/legal/siphasher-mit.txt",
      sha256: "79c66f20755bf42f245b6d06c531c4b03788a0af0a00064ffa4af82fd968f8d9",
    }),
    isarrayMit: Object.freeze({
      path: "scripts/legal/isarray-mit.txt",
      sha256: "f55049a90ff1d58d3ce9c2cdda21ee3b07d4cba4715448ae96e24736345f3e83",
    }),
    dingbatBsd: Object.freeze({
      path: "scripts/legal/dingbat-to-unicode-bsd-2-clause.txt",
      sha256: "1515bcd43043c17ef9d64339c48960308707a80ca7525844cba0724a4b4a6798",
    }),
  }),

  // Componentes cujo artefato publicado nao traz o texto de licenca. A chave e
  // `<nome>@<versao>`: fixar a versao impede que a excecao sobreviva em
  // silencio a uma atualizacao de dependencia.
  licenseFallbacks: Object.freeze({
    "siphasher@1.0.2": Object.freeze({
      ecosystem: "cargo",
      license: "MIT/Apache-2.0",
      fragments: Object.freeze(["siphasherMit"]),
      sourceRepository: "https://github.com/jedisct1/rust-siphash",
      revision: "db8172048a1c9bdef0dcec782d965c236161af13",
      revisionSource: ".cargo_vcs_info.json",
      licensePaths: Object.freeze([]),
      copyrightHolder:
        "The Rust Project Developers (2012-2016); Frank Denis (2016-2026)",
      copyrightBasis:
        "Linhas de copyright transcritas do arquivo COPYING publicado pelo proprio crate.",
      rationale:
        "O crate empacota apenas um COPYING de 281 bytes que APONTA para LICENSE-APACHE e LICENSE-MIT sem incluir nenhum dos dois, e no commit fixado o repositorio de origem tambem so publica esse mesmo ponteiro. Ponteiro nao cumpre a exigencia de reproduzir o texto na distribuicao. Texto canonico da MIT obtido da SPDX, precedido das linhas de copyright que o COPYING declara.",
    }),
    "isarray@1.0.0": Object.freeze({
      ecosystem: "npm",
      license: "MIT",
      fragments: Object.freeze(["isarrayMit"]),
      sourceRepository: "https://github.com/juliangruber/isarray",
      revision: "2a23a281f369e9ae06394c0fb4d2381355a6ba33",
      revisionSource: "commit da tag anotada v1.0.0",
      licensePaths: Object.freeze(["README.md"]),
      rationale:
        "O pacote npm nao inclui arquivo de licenca; o texto MIT com a linha de copyright do titular esta na secao License do README.md publicado.",
    }),
    "dingbat-to-unicode@1.0.1": Object.freeze({
      ecosystem: "npm",
      license: "BSD-2-Clause",
      fragments: Object.freeze(["dingbatBsd"]),
      sourceRepository: "https://github.com/mwilliamson/dingbat-to-unicode",
      revision: "46e2dfb2632019d18bd1fb2478d92494f6eab081",
      revisionSource: "commit de main inspecionado em 30/08/2026",
      licensePaths: Object.freeze([]),
      copyrightHolder: "Michael Williamson <mike@zwobble.org>",
      copyrightYear: "2021",
      copyrightBasis:
        "Titular: campo author do package.json publicado, que e tambem o unico autor dos commits. Ano: todo o historico do repositorio esta em janeiro de 2021, com 22 commits entre 16 e 23 de janeiro de 2021.",
      rationale:
        "Unico caso sem aviso publicado em lugar nenhum: nem o pacote npm nem o repositorio de origem trazem arquivo de licenca, e a busca de codigo do GitHub por copyright no repositorio retorna zero ocorrencias. A BSD-2-Clause exige que a redistribuicao binaria reproduza o aviso de copyright, e o modelo da SPDX com marcadores por preencher nao identificaria titular algum. O aviso foi instanciado a partir do que o proprio upstream declara, com a base registrada em copyrightBasis.",
    }),
    "webview2-com@0.38.2": Object.freeze({
      ecosystem: "cargo",
      license: "MIT",
      fragments: Object.freeze(["webview2Mit"]),
      sourceRepository: "https://github.com/wravery/webview2-rs",
      revision: "b74dc5e2b394044bea5191052868ce7a106c202c",
      revisionSource: ".cargo_vcs_info.json",
      licensePaths: Object.freeze(["LICENSE"]),
      rationale: "O crate publicado em crates.io nao inclui arquivo de licenca.",
    }),
    "webview2-com-sys@0.38.2": Object.freeze({
      ecosystem: "cargo",
      license: "MIT",
      fragments: Object.freeze(["webview2Mit"]),
      sourceRepository: "https://github.com/wravery/webview2-rs",
      revision: "b74dc5e2b394044bea5191052868ce7a106c202c",
      revisionSource: ".cargo_vcs_info.json",
      licensePaths: Object.freeze(["LICENSE"]),
      rationale: "O crate publicado em crates.io nao inclui arquivo de licenca.",
    }),
    "webview2-com-macros@0.8.1": Object.freeze({
      ecosystem: "cargo",
      license: "MIT",
      fragments: Object.freeze(["webview2Mit"]),
      sourceRepository: "https://github.com/wravery/webview2-rs",
      revision: "dffa41a8a46d3f5565eefbff2de57d38d399f158",
      revisionSource: ".cargo_vcs_info.json",
      licensePaths: Object.freeze(["LICENSE"]),
      rationale: "O crate publicado em crates.io nao inclui arquivo de licenca.",
    }),
    "unic-ucd-ident@0.9.0": Object.freeze({
      ecosystem: "cargo",
      license: "MIT/Apache-2.0",
      fragments: Object.freeze(["unicMit", "unicApache"]),
      sourceRepository: "https://github.com/open-i18n/rust-unic",
      revision: "8a6ce83063d90b91ae2ce59eddb803edd393fca9",
      revisionSource: ".cargo_vcs_info.json",
      licensePaths: Object.freeze(["LICENSE-MIT", "LICENSE-APACHE"]),
      rationale:
        "O crate publicado nao inclui arquivo de licenca. A expressao e uma opcao dupla; ambos os textos sao reproduzidos para nao antecipar a eleicao.",
    }),
    "unic-ucd-version@0.9.0": Object.freeze({
      ecosystem: "cargo",
      license: "MIT/Apache-2.0",
      fragments: Object.freeze(["unicMit", "unicApache"]),
      sourceRepository: "https://github.com/open-i18n/rust-unic",
      revision: "5878605364af97a3358368a6eaef02104af2e016",
      revisionSource: ".cargo_vcs_info.json",
      licensePaths: Object.freeze(["LICENSE-MIT", "LICENSE-APACHE"]),
      rationale: "O crate publicado nao inclui arquivo de licenca.",
    }),
    "unic-common@0.9.0": Object.freeze({
      ecosystem: "cargo",
      license: "MIT/Apache-2.0",
      fragments: Object.freeze(["unicMit", "unicApache"]),
      sourceRepository: "https://github.com/open-i18n/rust-unic",
      revision: "5878605364af97a3358368a6eaef02104af2e016",
      revisionSource: ".cargo_vcs_info.json",
      licensePaths: Object.freeze(["LICENSE-MIT", "LICENSE-APACHE"]),
      rationale: "O crate publicado nao inclui arquivo de licenca.",
    }),
    "unic-char-range@0.9.0": Object.freeze({
      ecosystem: "cargo",
      license: "MIT/Apache-2.0",
      fragments: Object.freeze(["unicMit", "unicApache"]),
      sourceRepository: "https://github.com/open-i18n/rust-unic",
      revision: "5878605364af97a3358368a6eaef02104af2e016",
      revisionSource: ".cargo_vcs_info.json",
      licensePaths: Object.freeze(["LICENSE-MIT", "LICENSE-APACHE"]),
      rationale: "O crate publicado nao inclui arquivo de licenca.",
    }),
    "unic-char-property@0.9.0": Object.freeze({
      ecosystem: "cargo",
      license: "MIT/Apache-2.0",
      fragments: Object.freeze(["unicMit", "unicApache"]),
      sourceRepository: "https://github.com/open-i18n/rust-unic",
      revision: "5878605364af97a3358368a6eaef02104af2e016",
      revisionSource: ".cargo_vcs_info.json",
      licensePaths: Object.freeze(["LICENSE-MIT", "LICENSE-APACHE"]),
      rationale: "O crate publicado nao inclui arquivo de licenca.",
    }),
    "selectors@0.36.1": Object.freeze({
      ecosystem: "cargo",
      license: "MPL-2.0",
      fragments: Object.freeze(["selectorsMpl"]),
      sourceRepository: "https://github.com/servo/stylo",
      revision: "635e1a19d02960588a00e189bd4bd5bdb150ec3d",
      revisionSource: ".cargo_vcs_info.json",
      licensePaths: Object.freeze([]),
      correspondingSource: "https://crates.io/crates/selectors/0.36.1",
      rationale:
        "Nem o crate nem a raiz do repositorio de origem publicam arquivo de licenca. A MPL 2.0 opera no modelo por arquivo e os 16 arquivos .rs do crate carregam o aviso do Exhibit A, que remete ao texto oficial da Mozilla, reproduzido no fragmento. A localizacao do codigo-fonte correspondente esta registrada acima.",
    }),
  }),
});

export default POLICY;
