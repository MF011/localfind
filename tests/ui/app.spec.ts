import { test, expect } from "@playwright/test";
test("browser mode is honest and themes and filters work", async ({ page }) => {
  await page.goto("/");
  await expect(
    page.getByText("Это веб-просмотр интерфейса.", { exact: false }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Индексировать все диски" }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "Переключить тему" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await page.getByRole("button", { name: "Фильтры", exact: true }).click();
  await expect(page.getByText("Изменён после")).toBeVisible();
  await page.keyboard.press("Control+k");
  await expect(
    page.getByRole("textbox", { name: "Поиск файлов" }),
  ).toBeFocused();
});
test("native IPC integration handles search, preview and pause", async ({
  page,
}) => {
  // Contract test only: native filesystem and Rust core are tested separately.
  await page.addInitScript(() => {
    const w = window as any;
    w.isTauri = true;
    w.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main" },
      },
      invoke: async (command: string, args: any) => {
        const ops: any = {
          status: {
            total: 1,
            pending: 1,
            processing: 0,
            ready: 0,
            errors: 0,
            unsupported: 0,
            offline: 0,
            bytes: 4096,
            paused: false,
            scanning: false,
            cancelled: false,
          },
          roots: [{ id: 1, path: "C:\\Документы", online: true, error: null }],
          settings: {
            theme: "light",
            ocr: "true",
            workers: "4",
            tesseract: "tesseract",
            tessdata: "",
          },
          search: [
            {
              id: 1,
              name: "Счёт AB-102.txt",
              path: "C:\\Документы\\Счёт AB-102.txt",
              size: 123,
              modified: 1700000000000000000,
              source: "text",
              location: "Страница 1",
              snippet: "Договор ⟦AB-102⟧",
              online: true,
            },
          ],
          preview: [
            {
              location: "Страница 1",
              source: "text",
              text: "Договор AB-102 <script>alert(1)</script>",
            },
          ],
        };
        return ops[args?.op] ?? null;
      },
    };
  });
  await page.goto("/");
  await page.getByRole("textbox", { name: "Поиск файлов" }).fill("AB-102");
  await expect(page.getByRole("listbox").getByRole("option")).toHaveCount(1);
  await expect(page.locator(".preview-content")).toContainText(
    "<script>alert(1)</script>",
  );
  await expect(page.locator(".preview-content script")).toHaveCount(0);
  await expect(page.locator(".preview-content mark")).toHaveText("AB-102");
  await page.getByRole("button", { name: "Пауза", exact: true }).click();
  await page.screenshot({ path: "../ui-search.png", fullPage: true });
});
