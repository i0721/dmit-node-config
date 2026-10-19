import Link from 'next/link';
export default function NotFound() {
  return (
    <main className="mx-auto max-w-xl p-8">
      <h1 className="text-2xl font-bold">页面不存在</h1>
      <Link className="text-brand underline" href="/">
        返回节点总览
      </Link>
    </main>
  );
}
