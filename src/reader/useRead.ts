// 从文献库里打开阅读器：外框（App.tsx）提供 read(附件 id)，列表与详情里调用。
import { createContext, useCallback, useContext } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { api } from "@/shared/ipc";
import { useUi } from "@/shared/ui";

export const ReadContext = createContext<(attachmentId: number) => void>(() => {});

/** read：在阅读器里打开某个 PDF 附件。
 *  openAttachment：能读就读，否则交给系统程序。openItem：打开文献的第一个能读的 PDF。 */
export function useRead() {
  const read = useContext(ReadContext);
  const qc = useQueryClient();
  const { notify } = useUi();
  const openAttachment = useCallback(
    async (id: number) => {
      try {
        const a = await qc.fetchQuery({ queryKey: ["attachment", id], queryFn: () => api.attachmentsGet(id) });
        if (a.readable) read(id);
        else await api.attachmentsOpen(id);
      } catch (e) {
        notify(String(e), "error");
      }
    },
    [qc, read, notify],
  );
  const openItem = useCallback(
    async (id: number) => {
      try {
        const d = await qc.fetchQuery({ queryKey: ["item", id], queryFn: () => api.itemsGet(id) });
        const pdf = d.attachments.find((a) => a.readable);
        if (pdf) read(pdf.id);
        else notify("这条文献没有能在 App 里阅读的 PDF");
      } catch (e) {
        notify(String(e), "error");
      }
    },
    [qc, read, notify],
  );
  return { read, openAttachment, openItem };
}
