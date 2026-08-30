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
  licenseFilePrefixes: Object.freeze([
    "license",
    "licence",
    "copying",
    "notice",
    "unlicense",
  ]),

  // Extensoes que nao carregam o texto da licenca e portanto nao contam como
  // aviso, mesmo quando o nome comeca com um dos prefixos acima.
  licenseFileIgnoredExtensions: Object.freeze([".spdx", ".json", ".xml"]),

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
    isarrayMit: Object.freeze({
      path: "scripts/legal/isarray-mit.txt",
      sha256: "f3c53688dd17844abee97bce2f14dea131de88e59d8b64dc168a4f77a1a35d08",
    }),
    dingbatBsd: Object.freeze({
      path: "scripts/legal/dingbat-to-unicode-bsd-2-clause.txt",
      sha256: "ab20668a96b81bcc92d490ed319110dca13c7d441aaaab7f194331927e5fbc85",
    }),
  }),

  // Componentes cujo artefato publicado nao traz o texto de licenca. A chave e
  // `<nome>@<versao>`: fixar a versao impede que a excecao sobreviva em
  // silencio a uma atualizacao de dependencia.
  licenseFallbacks: Object.freeze({
    "isarray@1.0.0": Object.freeze({
      ecosystem: "npm",
      license: "MIT",
      fragments: Object.freeze(["isarrayMit"]),
      sourceRepository: "https://github.com/juliangruber/isarray",
      revision: "v1.0.0",
      licensePaths: Object.freeze(["README.md"]),
      rationale:
        "O pacote npm nao inclui arquivo de licenca; o texto MIT com a linha de copyright do titular esta na secao License do README.md publicado.",
    }),
    "dingbat-to-unicode@1.0.1": Object.freeze({
      ecosystem: "npm",
      license: "BSD-2-Clause",
      fragments: Object.freeze(["dingbatBsd"]),
      sourceRepository: "https://github.com/mwilliamson/dingbat-to-unicode",
      revision: "main",
      licensePaths: Object.freeze([]),
      copyrightHolder: "Michael Williamson <mike@zwobble.org>",
      rationale:
        "Unico caso sem aviso publicado: nem o pacote npm nem o repositorio de origem trazem arquivo de licenca ou linha de copyright. Texto canonico da SPDX, com o titular declarado no campo author do package.json. O ano nao foi preenchido porque o upstream nao o declara.",
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
