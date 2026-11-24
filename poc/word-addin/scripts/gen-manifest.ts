// 生成 Word 侧载清单（add-in only XML manifest）。
// 用法：node scripts/gen-manifest.ts [基地址]，默认 http://localhost:3127
import { readFileSync, writeFileSync } from "node:fs";

const base = (process.argv[2] ?? "http://localhost:3127").replace(/\/$/, "");
const id = readFileSync(".addin-id", "utf8").trim();
const version = JSON.parse(readFileSync("package.json", "utf8")).version + ".0";

const xml = `<?xml version="1.0" encoding="UTF-8"?>
<OfficeApp xmlns="http://schemas.microsoft.com/office/appforoffice/1.1"
           xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
           xmlns:bt="http://schemas.microsoft.com/office/officeappbasictypes/1.0"
           xmlns:ov="http://schemas.microsoft.com/office/taskpaneappversionoverrides"
           xsi:type="TaskPaneApp">
  <Id>${id}</Id>
  <Version>${version}</Version>
  <ProviderName>Research Lab</ProviderName>
  <DefaultLocale>zh-CN</DefaultLocale>
  <DisplayName DefaultValue="Research Lab 引文 PoC"/>
  <Description DefaultValue="Phase 0-B：本地文献动态引用验证"/>
  <IconUrl DefaultValue="${base}/icons/icon-32.png"/>
  <HighResolutionIconUrl DefaultValue="${base}/icons/icon-64.png"/>
  <SupportUrl DefaultValue="${base}/taskpane.html"/>
  <AppDomains/>
  <Hosts>
    <Host Name="Document"/>
  </Hosts>
  <Requirements>
    <Sets DefaultMinVersion="1.1">
      <Set Name="WordApi" MinVersion="1.4"/>
    </Sets>
  </Requirements>
  <DefaultSettings>
    <SourceLocation DefaultValue="${base}/taskpane.html"/>
  </DefaultSettings>
  <Permissions>ReadWriteDocument</Permissions>
  <VersionOverrides xmlns="http://schemas.microsoft.com/office/taskpaneappversionoverrides" xsi:type="VersionOverridesV1_0">
    <Hosts>
      <Host xsi:type="Document">
        <DesktopFormFactor>
          <ExtensionPoint xsi:type="PrimaryCommandSurface">
            <OfficeTab id="TabHome">
              <Group id="RL.Group">
                <Label resid="RL.GroupLabel"/>
                <Icon>
                  <bt:Image size="16" resid="RL.Icon16"/>
                  <bt:Image size="32" resid="RL.Icon32"/>
                  <bt:Image size="80" resid="RL.Icon80"/>
                </Icon>
                <Control xsi:type="Button" id="RL.OpenPane">
                  <Label resid="RL.ButtonLabel"/>
                  <Supertip>
                    <Title resid="RL.ButtonLabel"/>
                    <Description resid="RL.ButtonTip"/>
                  </Supertip>
                  <Icon>
                    <bt:Image size="16" resid="RL.Icon16"/>
                    <bt:Image size="32" resid="RL.Icon32"/>
                    <bt:Image size="80" resid="RL.Icon80"/>
                  </Icon>
                  <Action xsi:type="ShowTaskpane">
                    <TaskpaneId>Office.AutoShowTaskpaneWithDocument</TaskpaneId>
                    <SourceLocation resid="RL.Taskpane.Url"/>
                  </Action>
                </Control>
              </Group>
            </OfficeTab>
          </ExtensionPoint>
        </DesktopFormFactor>
      </Host>
    </Hosts>
    <Resources>
      <bt:Images>
        <bt:Image id="RL.Icon16" DefaultValue="${base}/icons/icon-16.png"/>
        <bt:Image id="RL.Icon32" DefaultValue="${base}/icons/icon-32.png"/>
        <bt:Image id="RL.Icon80" DefaultValue="${base}/icons/icon-80.png"/>
      </bt:Images>
      <bt:Urls>
        <bt:Url id="RL.Taskpane.Url" DefaultValue="${base}/taskpane.html"/>
      </bt:Urls>
      <bt:ShortStrings>
        <bt:String id="RL.GroupLabel" DefaultValue="Research Lab"/>
        <bt:String id="RL.ButtonLabel" DefaultValue="引文"/>
      </bt:ShortStrings>
      <bt:LongStrings>
        <bt:String id="RL.ButtonTip" DefaultValue="打开 Research Lab 引文面板：插入引文、生成参考文献、切换样式"/>
      </bt:LongStrings>
    </Resources>
  </VersionOverrides>
</OfficeApp>
`;
writeFileSync("manifest.xml", xml);
console.log(`manifest.xml → ${base}（id ${id}，版本 ${version}）`);
