// 设置页的「联网补全」：开关、自动、联系邮箱、说明，以及「补全全部文献」。
import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Globe } from "lucide-react";
import { api, type MetadataSettingsPatch } from "@/shared/ipc";
import { useUi } from "@/shared/ui";
import { useMetadata } from "./MetadataProvider";

export function MetadataSettingsSection() {
  const qc = useQueryClient();
  const ui = useUi();
  const { ensureConsent } = useMetadata();
  const s = useQuery({ queryKey: ["metadataSettings"], queryFn: api.metadataSettingsGet }).data;
  const status = useQuery({ queryKey: ["lookupStatus"], queryFn: api.metadataStatus }).data;
  const [email, setEmail] = useState("");
  useEffect(() => setEmail(s?.email ?? ""), [s?.email]);

  const save = async (patch: MetadataSettingsPatch, ok?: string) => {
    try {
      await api.metadataSettingsSet(patch);
      if (ok) ui.notify(ok);
    } catch (e) {
      ui.notify(String(e), "error");
    }
    qc.invalidateQueries({ queryKey: ["metadataSettings"] });
  };
  const all = async () => {
    if (!(await ensureConsent())) return;
    try {
      const n = await api.metadataLookupAll();
      ui.notify(n ? `开始联网补全 ${n} 条（已经查过的会跳过）` : "库里没有带 DOI / ISBN 的文献");
    } catch (e) {
      ui.notify(String(e), "error");
    }
    qc.invalidateQueries({ queryKey: ["lookupStatus"] });
  };

  return (
    <div>
      <div className="mb-2 flex items-center justify-between">
        <h2 className="text-xs font-medium text-muted-foreground">联网补全</h2>
        <button
          onClick={() => void all()}
          disabled={s?.enabled === false || status?.running}
          className="flex items-center gap-1 rounded-md border px-2.5 py-1 text-xs hover:bg-muted disabled:opacity-50"
        >
          <Globe size={13} />
          {status?.running ? `正在查 ${status.done} / ${status.total}` : "补全全部文献"}
        </button>
      </div>
      <p className="mb-3 text-xs leading-relaxed text-muted-foreground">
        按 DOI / ISBN 联网查出版信息：只补空着的字段，和现有不同的放进侧栏「待确认」。DOI 发给 doi.org（再转到 Crossref、DataCite 等），带下面的联系邮箱；ISBN 发给 Open Library，不带邮箱。只发 DOI / ISBN 本身。
      </p>
      <dl className="grid grid-cols-[110px_1fr] items-center gap-x-3 gap-y-2">
        <dt className="text-muted-foreground">联网</dt>
        <dd className="flex items-center gap-2">
          <label className="flex items-center gap-1.5">
            <input
              type="checkbox"
              checked={s?.enabled === true}
              onChange={(e) => void (e.target.checked && s?.enabled === null ? ensureConsent() : save({ enabled: e.target.checked }))}
            />
            {s?.enabled === null ? "还没打开（第一次用时会先问你）" : s?.enabled ? "已打开" : "已关闭"}
          </label>
        </dd>
        <dt className="text-muted-foreground">自动查</dt>
        <dd>
          <label className="flex items-center gap-1.5">
            <input type="checkbox" checked={s?.auto ?? true} disabled={s?.enabled !== true} onChange={(e) => void save({ auto: e.target.checked })} />
            导入后、填写 DOI / ISBN 后自动查
          </label>
        </dd>
        <dt className="text-muted-foreground">联系邮箱</dt>
        <dd className="flex items-center gap-2">
          <input
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            placeholder="可留空"
            className="selectable min-w-0 flex-1 rounded-md border bg-background px-2 py-1 text-xs outline-none focus:border-ring"
          />
          <button
            onClick={() => void save({ email }, email.trim() ? "已保存邮箱" : "已清除邮箱")}
            disabled={email.trim() === (s?.email ?? "")}
            className="shrink-0 rounded-md border px-2.5 py-1 text-xs hover:bg-muted disabled:opacity-40"
          >
            保存
          </button>
        </dd>
      </dl>
    </div>
  );
}
