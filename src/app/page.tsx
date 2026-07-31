import { redirect } from "next/navigation";

/** 静态站点根路径只负责把首次访问引导到默认的代理控制台。 */

export default function Home() {
  redirect("/console");
}
