export type FinalHtmlSemanticExpectation = {
  selector: string;
  text?: string;
  attributes?: Readonly<Record<string, string>>;
};

export type FinalHtmlFixture = {
  id:
    | "artigo-academico"
    | "referencias-abnt"
    | "tabela"
    | "figura"
    | "imagem-redimensionada"
    | "youtube"
    | "task-list"
    | "mention"
    | "markdown"
    | "markdown-html"
    | "pdf-derived";
  label: string;
  html: string;
  expectations: readonly FinalHtmlSemanticExpectation[];
};

/**
 * Golden semantic fixtures required by the MainSite compatibility contract.
 * They deliberately contain editor-only or unsafe attributes in a few places
 * so the tests prove meaning survives while the final sanitizer narrows the
 * persisted HTML to the reviewed MainSite allowlist.
 */
export const FINAL_HTML_FIXTURES: readonly FinalHtmlFixture[] = [
  {
    id: "artigo-academico",
    label: "artigo acadêmico com estrutura semântica",
    html: `<article data-editor-only="true">
      <h1 style="text-align: center">Método e resultados</h1>
      <p style="text-align: justify; line-height: 1.5">Este estudo apresenta <strong>evidência verificável</strong> e <em>limitações explícitas</em>.</p>
      <h2>Conclusão</h2>
      <blockquote cite="https://example.com/metodo">A conclusão decorre dos dados observados.</blockquote>
    </article>`,
    expectations: [
      { selector: "h1", text: "Método e resultados" },
      { selector: "p strong", text: "evidência verificável" },
      { selector: "p em", text: "limitações explícitas" },
      { selector: "blockquote", text: "A conclusão decorre" },
    ],
  },
  {
    id: "referencias-abnt",
    label: "referências ABNT com links",
    html: `<h2>Referências</h2>
      <ol>
        <li>SILVA, Ana. <em>Pesquisa aplicada</em>. São Paulo: Exemplo, 2026. <a href="https://example.com/referencia" title="Fonte primária">Disponível em</a>.</li>
        <li>BRASIL. <strong>Norma editorial</strong>. <a href="mailto:editor@example.com">Contato editorial</a>.</li>
      </ol>`,
    expectations: [
      { selector: "h2", text: "Referências" },
      {
        selector: 'a[href="https://example.com/referencia"]',
        text: "Disponível em",
        attributes: { target: "_blank", rel: "noopener noreferrer" },
      },
      {
        selector: 'a[href="mailto:editor@example.com"]',
        text: "Contato editorial",
        attributes: { target: "_blank", rel: "noopener noreferrer" },
      },
    ],
  },
  {
    id: "tabela",
    label: "artigo com tabela complexa",
    html: `<h2>Resultados tabulados</h2>
      <table width="100%" style="width: 100%; border-collapse: collapse">
        <caption>Amostra por período</caption>
        <colgroup span="2" width="100%"><col span="1" style="width: 40%"><col span="1" style="width: 60%"></colgroup>
        <thead><tr><th scope="col">Período</th><th scope="col">Resultado</th></tr></thead>
        <tbody><tr><td rowspan="2">2026</td><td>42</td></tr><tr><td>47</td></tr></tbody>
        <tfoot><tr><td colspan="2">Total: 89</td></tr></tfoot>
      </table>`,
    expectations: [
      { selector: "table caption", text: "Amostra por período" },
      { selector: 'thead th[scope="col"]', text: "Período" },
      { selector: 'tbody td[rowspan="2"]', text: "2026" },
      { selector: 'tfoot td[colspan="2"]', text: "Total: 89" },
    ],
  },
  {
    id: "figura",
    label: "figura semântica com legenda",
    html: `<figure class="tiptap-figure" style="width: 80%">
      <img src="https://example.com/grafico.png" alt="Gráfico de evolução" title="Evolução anual">
      <figcaption>Figura 1 — Evolução anual da amostra.</figcaption>
    </figure>`,
    expectations: [
      { selector: "figure.tiptap-figure" },
      {
        selector: 'figure img[src="https://example.com/grafico.png"]',
        attributes: { alt: "Gráfico de evolução", loading: "lazy" },
      },
      { selector: "figure figcaption", text: "Figura 1 — Evolução anual" },
    ],
  },
  {
    id: "imagem-redimensionada",
    label: "imagem independente redimensionada",
    html: `<p style="text-align: center"><img src="https://example.com/mapa.png" alt="Mapa da amostra" data-width="65%" width="65%" style="width: 65%; max-width: 100%; object-fit: cover" loading="eager"></p>`,
    expectations: [
      {
        selector: 'img[src="https://example.com/mapa.png"]',
        attributes: { "data-width": "65%", width: "65%", loading: "eager" },
      },
    ],
  },
  {
    id: "youtube",
    label: "incorporação segura do YouTube",
    html: `<div data-youtube-video="" class="youtube-wrapper">
      <iframe src="https://www.youtube-nocookie.com/embed/abc123" title="Vídeo de demonstração" width="560" height="315" frameborder="0" allow="accelerometer; autoplay; encrypted-media" allowfullscreen></iframe>
    </div>`,
    expectations: [
      { selector: "div[data-youtube-video]" },
      {
        selector: 'iframe[src="https://www.youtube-nocookie.com/embed/abc123"]',
        attributes: { title: "Vídeo de demonstração", width: "560", height: "315" },
      },
    ],
  },
  {
    id: "task-list",
    label: "lista de tarefas TipTap",
    html: `<ul data-type="taskList">
      <li data-type="taskItem" data-checked="true"><label><input type="checkbox" checked disabled><span></span></label><div><p>Validar as fontes</p></div></li>
      <li data-type="taskItem" data-checked="false"><label><input type="checkbox" disabled><span></span></label><div><p>Publicar o artigo</p></div></li>
    </ul>`,
    expectations: [
      { selector: 'ul[data-type="taskList"]' },
      { selector: 'li[data-type="taskItem"][data-checked="true"]', text: "Validar as fontes" },
      { selector: 'input[type="checkbox"][checked][disabled]' },
      { selector: 'li[data-checked="false"]', text: "Publicar o artigo" },
    ],
  },
  {
    id: "mention",
    label: "menção editorial",
    html: `<p>Revisão atribuída a <span class="editor-mention" data-id="lcv-leo">@lcv-leo</span>.</p>`,
    expectations: [
      {
        selector: "span.editor-mention",
        text: "@lcv-leo",
        attributes: { class: "editor-mention" },
      },
    ],
  },
  {
    id: "markdown",
    label: "artigo importado de Markdown",
    html: `<h1>Relatório importado</h1>
      <p style="text-align: justify; text-indent: 1.5rem">Parágrafo convertido de Markdown com <strong>ênfase</strong>.</p>
      <ul><li>Primeiro achado</li><li>Segundo achado</li></ul>`,
    expectations: [
      { selector: "h1", text: "Relatório importado" },
      { selector: "p strong", text: "ênfase" },
      { selector: "ul li", text: "Primeiro achado" },
    ],
  },
  {
    id: "markdown-html",
    label: "artigo importado de Markdown com HTML",
    html: `<h1>Relatório híbrido</h1>
      <p style="text-align: justify">Texto Markdown preservado.</p>
      <blockquote cite="https://example.com/evidencia"><strong>Bloco HTML autorizado</strong></blockquote>
      <a href="/metodologia">Metodologia interna</a>`,
    expectations: [
      { selector: "h1", text: "Relatório híbrido" },
      { selector: "blockquote strong", text: "Bloco HTML autorizado" },
      {
        selector: 'a[href="/metodologia"]',
        attributes: { target: "_blank", rel: "noopener noreferrer" },
      },
    ],
  },
  {
    id: "pdf-derived",
    label: "artigo derivado de PDF",
    html: `<h1>Documento técnico convertido</h1>
      <p><strong>Página 1.</strong> Introdução extraída do documento original.</p>
      <hr>
      <h2>Dados recuperados</h2>
      <p>O conteúdo mantém ordem de leitura e separação semântica.</p>`,
    expectations: [
      { selector: "h1", text: "Documento técnico convertido" },
      { selector: "p strong", text: "Página 1." },
      { selector: "hr" },
      { selector: "h2", text: "Dados recuperados" },
    ],
  },
];
