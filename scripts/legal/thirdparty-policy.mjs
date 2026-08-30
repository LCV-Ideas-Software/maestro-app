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
      // Plataforma do ARTEFATO, nao da maquina que roda o gerador. Filtrar
      // pelo host faria o resultado depender de onde o comando foi executado:
      // no Linux omitiria uma dependencia restrita a win32 que embarca, e
      // incluiria uma restrita a linux que nao embarca. O grafo Cargo ja e
      // filtrado pelo alvo; o npm passa a ser tambem.
      targetOs: "win32",
      targetCpu: "x64",
      // npm documenta `libc` como restricao de plataforma no Linux. Como o
      // alvo aqui e Windows, nenhuma restricao de libc se aplica.
      targetLibc: null,
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
  // A arvore completa e analisada pelo parser SPDX oficial usado pelo npm. A
  // eleicao automatica so ocorre quando UM identificador desta ordem satisfaz a
  // expressao inteira sozinho e o corpo correspondente acompanha o artefato.
  // Parenteses nao mudam essa regra; AND, WITH e `+` permanecem semanticamente
  // distintos. Quando nenhuma folha preferida basta, `licenseElections` precisa
  // registrar a escolha completa e o gate valida todas as folhas oferecidas.
  // So entram aqui identificadores cujo texto e distinguivel dos demais por um
  // marcador proprio. MIT-0 e 0BSD ficaram DE FORA de proposito: o texto deles
  // difere de MIT e de ISC por uma clausula que o outro tem e eles nao, e
  // ausencia nao se detecta com busca de trecho. Eleger um por engano
  // afirmaria uma condicao de atribuicao que o texto nao traz. Quando uma
  // expressao so oferecer esses, o gate reprova e pede eleicao explicita.
  licenseElectionPreference: Object.freeze([
    "MIT",
    "ISC",
    "BSD-3-Clause",
    "Apache-2.0",
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
    // A clausula de atribuicao e o que separa MIT de MIT-0: a MIT exige que o
    // aviso acompanhe as copias, a MIT-0 nao. Usar so "Permission is hereby
    // granted" faria um texto MIT-0 corroborar MIT e vice-versa.
    MIT: Object.freeze([
      "The above copyright notice and this permission notice shall be included",
    ]),
    // Precisa ser frase que so existe no CORPO da licenca. "Apache License" e
    // a URL aparecem tambem em arquivos que apenas APONTAM para a licenca sem
    // reproduzi-la, e aceitar isso faria o gate corroborar um ponteiro.
    "Apache-2.0": Object.freeze([
      "TERMS AND CONDITIONS FOR USE, REPRODUCTION, AND DISTRIBUTION",
    ]),
    // BSD-2-Clause nao tem marcador positivo: todo o seu corpo substancial
    // tambem aparece na BSD-3-Clause, que apenas acrescenta a clausula de
    // nao-endosso. Distingui-las exige inspecao manual do artefato exato.
    // A clausula de nao-endosso e o que separa BSD-3-Clause de BSD-2-Clause.
    // Nao se ancora no comeco dela: os pacotes substituem ali o nome do titular
    // e pluralizam ("Neither the names of the Mozilla Foundation nor..."), o
    // que fazia o BSD-3 legitimo do source-map-js passar por ausente. O trecho
    // abaixo e literal no texto canonico e nas variantes, e nao existe no
    // BSD-2-Clause, que nao tem clausula de nao-endosso nenhuma.
    "BSD-3-Clause": Object.freeze([
      "may be used to endorse or promote products derived from this software without specific prior written permission",
    ]),
    // Mesma razao: a 0BSD e a ISC sem a condicao de manter o aviso nas copias.
    // O marcador da ISC e justamente essa condicao.
    ISC: Object.freeze([
      "provided that the above copyright notice and this permission notice appear in all copies",
    ]),
    "CC0-1.0": Object.freeze([
      "Affirmer hereby overtly, fully, permanently, irrevocably and unconditionally waives, abandons, and surrenders all of Affirmer's Copyright and Related Rights",
    ]),
    // Titulo, identificador e URL podem existir num arquivo que apenas aponta
    // para a licenca. Esta frase pertence ao corpo concessivo da Unicode-3.0.
    "Unicode-3.0": Object.freeze([
      "Permission is hereby granted, free of charge, to any person obtaining a copy of data files",
    ]),
    Unlicense: Object.freeze([
      "This is free and unencumbered software released into the public domain",
    ]),
    Zlib: Object.freeze([
      "This software is provided 'as-is'",
      "altered source versions must be plainly marked",
    ]),
    "MPL-2.0": Object.freeze([
      "All distribution of Covered Software in Source Code Form, including any Modifications that You create or to which You contribute, must be under the terms of this License",
    ]),
    "BSL-1.0": Object.freeze([
      "to use, reproduce, display, distribute, execute, and transmit the Software, and to prepare derivative works of the Software",
    ]),
    "Python-2.0": Object.freeze([
      "PSF hereby grants Licensee a nonexclusive, royalty-free, world-wide license to reproduce, analyze, test, perform and/or display publicly",
    ]),
    "CDLA-Permissive-2.0": Object.freeze([
      "A Data Recipient may use, modify, and share the Data made available by Data Provider(s) under this agreement",
    ]),
  }),

  // Inspecoes manuais para declaracoes que nao sao identificadores SPDX ou
  // cuja identidade nao pode ser provada por substring positiva, como a
  // BSD-2-Clause (subconjunto textual da BSD-3-Clause). Cada registro fixa a
  // origem exata; uma dependencia git/path ou outro tarball com as mesmas
  // coordenadas nao herda a inspecao.
  unverifiableLicenseDeclarations: Object.freeze({
    "duck@0.1.12": Object.freeze({
      ecosystem: "npm",
      source: "https://registry.npmjs.org/duck/-/duck-0.1.12.tgz",
      declared: "BSD",
      identifiedLicense: "BSD-2-Clause",
      rationale:
        "O pacote declara apenas BSD, que nao e identificador SPDX: nao distingue BSD-2-Clause de BSD-3-Clause nem das demais variantes, e portanto nenhum marcador pode confirma-lo. Inspecionado a mao em 30/08/2026: o LICENSE empacotado traz o texto BSD de duas clausulas, com as duas condicoes numeradas de redistribuicao e sem a clausula de nao-endosso, sob copyright de Michael Williamson (2013). O texto acompanha o artefato integralmente; o que falta e a precisao do identificador declarado pelo publicador, nao o aviso.",
    }),
    "dingbat-to-unicode@1.0.1": Object.freeze({
      ecosystem: "npm",
      source:
        "https://registry.npmjs.org/dingbat-to-unicode/-/dingbat-to-unicode-1.0.1.tgz",
      declared: "BSD-2-Clause",
      identifiedLicense: "BSD-2-Clause",
      rationale:
        "Inspecionado em 30/08/2026: o fragmento vendorizado instancia as duas condicoes de redistribuicao da BSD-2-Clause e nao contem clausula de nao-endosso. A proveniencia do titular e do ano permanece registrada no fallback do mesmo artefato.",
    }),
    "entities@4.5.0": Object.freeze({
      ecosystem: "npm",
      source: "https://registry.npmjs.org/entities/-/entities-4.5.0.tgz",
      declared: "BSD-2-Clause",
      identifiedLicense: "BSD-2-Clause",
      rationale:
        "Inspecionado em 30/08/2026: o LICENSE do tarball contem exatamente as duas condicoes de redistribuicao e nao contem a terceira clausula de nao-endosso da BSD-3-Clause.",
    }),
    "lop@0.4.2": Object.freeze({
      ecosystem: "npm",
      source: "https://registry.npmjs.org/lop/-/lop-0.4.2.tgz",
      declared: "BSD-2-Clause",
      identifiedLicense: "BSD-2-Clause",
      rationale:
        "Inspecionado em 30/08/2026: o LICENSE do tarball enumera somente as duas condicoes de redistribuicao e nao contem clausula de nao-endosso.",
    }),
    "mammoth@1.12.1": Object.freeze({
      ecosystem: "npm",
      source: "https://registry.npmjs.org/mammoth/-/mammoth-1.12.1.tgz",
      declared: "BSD-2-Clause",
      identifiedLicense: "BSD-2-Clause",
      rationale:
        "Inspecionado em 30/08/2026: o LICENSE do tarball enumera somente as duas condicoes de redistribuicao e nao contem clausula de nao-endosso.",
    }),
    "option@0.2.4": Object.freeze({
      ecosystem: "npm",
      source: "https://registry.npmjs.org/option/-/option-0.2.4.tgz",
      declared: "BSD-2-Clause",
      identifiedLicense: "BSD-2-Clause",
      rationale:
        "Inspecionado em 30/08/2026: o LICENSE do tarball enumera somente as duas condicoes de redistribuicao e nao contem clausula de nao-endosso.",
    }),
  }),

  // Eleicoes explicitas, por `<nome>@<versao>`. Cada chave aceita o objeto
  // historico unico ou uma lista de objetos quando as mesmas coordenadas
  // identificam mais de um artefato distribuido; a selecao sempre usa o par
  // exato `ecosystem` + `source`, nunca a primeira entrada da lista. Necessarias
  // quando nenhuma licenca preferida satisfaz sozinha a arvore completa ou quando uma decisao
  // auditada deve prevalecer sobre a preferencia automatica. Fixar a versao
  // impede que a decisao sobreviva em silencio a uma atualizacao de dependencia.
  //
  // `expression` e conferida contra o que o pacote declara: entrada obsoleta ou
  // com erro de digitacao reprova em vez de aplicar uma escolha que o pacote
  // nunca ofereceu. `ecosystem` e `source` amarram a decisao ao artefato exato
  // resolvido pelo lockfile; um fork git/path com as mesmas coordenadas nao
  // herda uma leitura feita sobre o pacote do registro oficial.
  // Textos ACRESCENTADOS ao que o pacote ja traz, nunca em substituicao.
  // Serve ao caso em que a expressao e conjuntiva e o pacote reproduz apenas
  // parte das licencas exigidas: o texto proprio dele continua no aviso, e o
  // que falta e completado a partir de origem registrada.
  licenseSupplements: Object.freeze({
    "pako@1.0.11": Object.freeze({
      ecosystem: "npm",
      fragments: Object.freeze(["pakoZlib"]),
      // Mesma exigencia dos fallbacks: texto vendorizado tem de apontar para
      // uma revisao imutavel, senao o sha256 prova apenas que o arquivo local
      // nao mudou, nunca de que bytes de upstream ele saiu.
      sourceRepository: "https://github.com/spdx/license-list-data",
      revision: "c4a7237ec8f4654e867546f9f409749300f1bf4c",
      revisionSource: "commit da tag v3.28.0 da lista oficial SPDX",
      sourcePath: "text/Zlib.txt",
      rationale:
        "A expressao (MIT AND Zlib) e conjuntiva: as duas licencas valem ao mesmo tempo. O pacote publica um unico LICENSE com o texto MIT e o copyright dos autores, e nao acompanha o texto da Zlib. O pako e um porto em JavaScript do zlib, o que explica a conjuncao. O LICENSE proprio segue reproduzido; o texto da Zlib e acrescentado a ele.",
    }),
  }),

  licenseElections: Object.freeze({
    "pako@1.0.11": Object.freeze({
      ecosystem: "npm",
      source: "https://registry.npmjs.org/pako/-/pako-1.0.11.tgz",
      expression: "(MIT AND Zlib)",
      elected: "MIT AND Zlib",
      rationale:
        "Expressao conjuntiva: nao ha escolha a fazer, as duas licencas se aplicam. Registrada para que o gate confirme que os dois textos acompanham o artefato, o que so passou a ser verdade com o complemento declarado em licenseSupplements.",
    }),
    "serial2@0.2.37": Object.freeze({
      ecosystem: "cargo",
      source: "registry+https://github.com/rust-lang/crates.io-index",
      expression: "BSD-2-Clause OR Apache-2.0",
      elected: "BSD-2-Clause",
      manualTextInspection: Object.freeze({
        identifiedLicenses: Object.freeze(["BSD-2-Clause"]),
        rationale:
          "O LICENSE-BSD do crate tem as duas condicoes da BSD-2-Clause e nao contem a clausula de nao-endosso da BSD-3-Clause; inspecionado no artefato crates.io fixado por ecosystem e source acima.",
      }),
      rationale:
        "BSD-2-Clause saiu da eleicao automatica porque seu texto e subconjunto do BSD-3-Clause: nenhum marcador o distingue, e um componente que oferecesse ambos e empacotasse so o BSD-3 corroboraria BSD-2 falsamente. Aqui a decisao foi verificada a mao em 30/08/2026: o crate publica LICENSE-BSD e LICENSE-APACHE, e o LICENSE-BSD nao contem a clausula de nao-endosso que caracteriza o BSD-3, sendo portanto BSD-2 de fato.",
    }),
    "dpi@0.1.2": Object.freeze({
      ecosystem: "cargo",
      source: "registry+https://github.com/rust-lang/crates.io-index",
      expression: "Apache-2.0 AND MIT",
      elected: "Apache-2.0 AND MIT",
      rationale:
        "Expressao conjuntiva: as duas licencas se aplicam. O crate reproduz ambos os textos, em LICENSE e LICENSE-LIBM-MIT, verificado em 30/08/2026.",
    }),
    "ring@0.17.14": Object.freeze({
      ecosystem: "cargo",
      source: "registry+https://github.com/rust-lang/crates.io-index",
      expression: "Apache-2.0 AND ISC",
      elected: "Apache-2.0 AND ISC",
      rationale:
        "Expressao conjuntiva: as duas licencas se aplicam. O crate reproduz ambos os textos, em LICENSE-BoringSSL e LICENSE-other-bits, com o LICENSE da raiz servindo de sumario que indica qual codigo veio sob qual delas. Verificado em 30/08/2026.",
    }),
    "siphasher@1.0.2": Object.freeze({
      ecosystem: "cargo",
      source: "registry+https://github.com/rust-lang/crates.io-index",
      expression: "MIT/Apache-2.0",
      elected: "MIT",
      rationale:
        "O crate nao reproduz nenhuma das duas licencas, so um ponteiro para elas, e o upstream tambem nao. Elege-se MIT, primeira da ordem de preferencia entre as oferecidas, com o texto vendorizado em scripts/legal/siphasher-mit.txt.",
    }),
    "dunce@1.0.5": Object.freeze({
      ecosystem: "cargo",
      source: "registry+https://github.com/rust-lang/crates.io-index",
      expression: "CC0-1.0 OR MIT-0 OR Apache-2.0",
      elected: "CC0-1.0",
      rationale:
        "A ordem de preferencia elegeria Apache-2.0, mas o crate empacota um unico LICENSE, e o texto nele e o da CC0-1.0. Eleger uma licenca cujo texto nao acompanha o artefato produziria afirmacao falsa; elege-se a que esta efetivamente reproduzida.",
    }),
    "dompurify@3.4.14": Object.freeze({
      ecosystem: "npm",
      source:
        "https://registry.npmjs.org/dompurify/-/dompurify-3.4.14.tgz",
      expression: "(MPL-2.0 OR Apache-2.0)",
      elected: "Apache-2.0",
      rationale:
        "Elege-se explicitamente Apache-2.0: e permissiva e evita as obrigacoes de arquivo da MPL-2.0 sobre um componente que e embutido no bundle distribuido. O registro fixa a decisao auditada mesmo que a ordem automatica mude.",
    }),
    "jszip@3.10.1": Object.freeze({
      ecosystem: "npm",
      source: "https://registry.npmjs.org/jszip/-/jszip-3.10.1.tgz",
      expression: "(MIT OR GPL-3.0-or-later)",
      elected: "MIT",
      rationale:
        "Elege-se explicitamente MIT por ser a opcao permissiva: nao acrescenta obrigacao reciproca ao trabalho combinado nem estende termos de copyleft a quem recebe o executavel. O registro fixa a decisao auditada mesmo que a ordem automatica mude.",
      correction:
        "Uma versao anterior desta justificativa afirmava que a aplicacao e proprietaria e que GPL-3.0-or-later seria incompativel. As duas afirmacoes sao falsas. Este repositorio e AGPL-3.0-or-later, e a secao 13 do LICENSE empacotado (linha 540, 'Remote Network Interaction; Use with the GNU General Public License') permite expressamente a combinacao com GPLv3. A eleicao de MIT permanece, mas pelo motivo correto acima, nao por incompatibilidade inexistente.",
    }),
    "unicode-ident@1.0.24": Object.freeze({
      ecosystem: "cargo",
      source: "registry+https://github.com/rust-lang/crates.io-index",
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
    pakoZlib: Object.freeze({
      path: "scripts/legal/pako-zlib.txt",
      sha256: "83a285c17ffd2e004b435a022b7e50dff6952c6851aaf511cb47e673494f063f",
    }),
    siphasherMit: Object.freeze({
      path: "scripts/legal/siphasher-mit.txt",
      sha256: "a187852b35120115cbbe8eb2484f43e4b89dc027629107e53e955ce7fa9d8371",
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
      textSourceRepository: "https://github.com/spdx/license-list-data",
      textRevision: "c4a7237ec8f4654e867546f9f409749300f1bf4c",
      textRevisionSource: "commit da tag v3.28.0 da lista oficial SPDX",
      textSourcePath: "text/MIT.txt",
      copyrightSourceRepository:
        "https://github.com/jedisct1/rust-siphash",
      copyrightRevision: "db8172048a1c9bdef0dcec782d965c236161af13",
      copyrightRevisionSource: ".cargo_vcs_info.json",
      copyrightSourcePath: "COPYING",
      licensePaths: Object.freeze([]),
      copyrightHolder:
        "The Rust Project Developers (2012-2016); Frank Denis (2016-2026)",
      copyrightBasis:
        "Linhas de copyright transcritas do arquivo COPYING publicado pelo proprio crate.",
      rationale:
        "O crate empacota apenas um COPYING de 281 bytes que APONTA para LICENSE-APACHE e LICENSE-MIT sem incluir nenhum dos dois, e na revisao fixada para o copyright o repositorio do crate tambem so publica esse mesmo ponteiro. Ponteiro nao cumpre a exigencia de reproduzir o texto na distribuicao. O texto canonico MIT vem do caminho e commit fixos da lista oficial SPDX; as linhas de copyright vem separadamente do COPYING do crate.",
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
