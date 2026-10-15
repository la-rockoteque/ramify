// A small markdown renderer for specs, QA plans and reviews. It builds DOM nodes and sets text
// through textContent only: the files come from a branch, so nothing in them becomes HTML.
// Headings, paragraphs, lists (with task boxes), fenced code, quotes, rules, pipe tables, and
// inline code, bold, italics and links. A leading YAML frontmatter folds into a <details>.
// ponytail: one list level — an indented item joins the item above it.
(() => {
  function el(tag, cls, ...kids) {
    const n = document.createElement(tag);
    if (cls) n.className = cls;
    for (const k of kids) if (k != null) n.append(k);
    return n;
  }

  const INLINE = /(`+)([^`]|[^`][\s\S]*?[^`])\1(?!`)|\*\*([^*]+?)\*\*|__([^_]+?)__|\*([^*\s][^*]*?)\*|(?<![A-Za-z0-9])_([^_\s][^_]*?)_(?![A-Za-z0-9])|\[([^\]]+)\]\(([^)\s]+)\)/g;

  function inline(text) {
    const frag = document.createDocumentFragment();
    let last = 0;
    for (const m of text.matchAll(INLINE)) {
      if (m.index > last) frag.append(text.slice(last, m.index));
      if (m[2] != null) frag.append(el('code', null, m[2]));
      else if (m[3] != null || m[4] != null) frag.append(el('strong', null, inline(m[3] ?? m[4])));
      else if (m[5] != null || m[6] != null) frag.append(el('em', null, inline(m[5] ?? m[6])));
      else {
        // A link shows its text and keeps the target as a tooltip: the window never navigates.
        const a = el('a', null, inline(m[7]));
        a.title = m[8];
        frag.append(a);
      }
      last = m.index + m[0].length;
    }
    if (last < text.length) frag.append(text.slice(last));
    return frag;
  }

  const cells = (row) => row.trim().replace(/^\|/, '').replace(/\|$/, '').split(/(?<!\\)\|/).map((c) => c.trim().replace(/\\\|/g, '|'));
  const isRule = (l) => /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/.test(l);
  const LIST = /^(\s*)([-*+]|\d+[.)])\s+(.*)$/;

  function render(src) {
    const out = document.createDocumentFragment();
    let lines = src.replace(/\r\n/g, '\n').split('\n');

    if (lines[0] === '---') {
      const end = lines.indexOf('---', 1);
      if (end > 0) {
        const pre = el('pre', null, el('code', null, lines.slice(1, end).join('\n')));
        out.append(el('details', null, el('summary', null, 'frontmatter'), pre));
        lines = lines.slice(end + 1);
      }
    }

    let i = 0;
    while (i < lines.length) {
      const line = lines[i];
      if (!line.trim()) { i++; continue; }

      const fence = line.match(/^\s*(```+|~~~+)(.*)$/);
      if (fence) {
        const body = [];
        for (i++; i < lines.length && !lines[i].trim().startsWith(fence[1]); i++) body.push(lines[i]);
        i++;
        out.append(el('pre', null, el('code', null, body.join('\n'))));
        continue;
      }

      const h = line.match(/^(#{1,6})\s+(.*?)\s*#*\s*$/);
      if (h) { out.append(el(`h${h[1].length}`, null, inline(h[2]))); i++; continue; }

      if (/^\s*([-*_])(\s*\1){2,}\s*$/.test(line)) { out.append(el('hr')); i++; continue; }

      if (line.includes('|') && i + 1 < lines.length && isRule(lines[i + 1])) {
        const table = el('table');
        table.append(el('thead', null, el('tr', null, ...cells(line).map((c) => el('th', null, inline(c))))));
        const tbody = el('tbody');
        for (i += 2; i < lines.length && lines[i].includes('|') && lines[i].trim(); i++) {
          tbody.append(el('tr', null, ...cells(lines[i]).map((c) => el('td', null, inline(c)))));
        }
        table.append(tbody);
        out.append(table);
        continue;
      }

      if (/^\s*>/.test(line)) {
        const body = [];
        for (; i < lines.length && /^\s*>/.test(lines[i]); i++) body.push(lines[i].replace(/^\s*>\s?/, ''));
        out.append(el('blockquote', null, render(body.join('\n'))));
        continue;
      }

      const item = line.match(LIST);
      if (item) {
        const list = el(/\d/.test(item[2]) ? 'ol' : 'ul');
        let li = null;
        for (; i < lines.length; i++) {
          const m = lines[i].match(LIST);
          if (m && m[1].length < 2) {
            const task = m[3].match(/^\[([ xX])\]\s+(.*)$/);
            li = el('li', task ? 'task' : null);
            if (task) {
              const box = el('input');
              box.type = 'checkbox';
              box.disabled = true;
              box.checked = task[1] !== ' ';
              li.append(box, ' ', inline(task[2]));
            } else li.append(inline(m[3]));
            list.append(li);
          } else if (lines[i].trim() && /^\s/.test(lines[i]) && li) {
            li.append(el('br'), inline(lines[i].trim().replace(/^([-*+]|\d+[.)])\s+/, '• ')));
          } else break;
        }
        out.append(list);
        continue;
      }

      const para = [];
      for (; i < lines.length && lines[i].trim() && !LIST.test(lines[i]) && !/^(#{1,6}\s|\s*```|\s*~~~|\s*>)/.test(lines[i])
        && !(lines[i].includes('|') && i + 1 < lines.length && isRule(lines[i + 1])); i++) para.push(lines[i].trim());
      out.append(el('p', null, inline(para.join(' '))));
    }
    return out;
  }

  window.markdown = render;
})();
