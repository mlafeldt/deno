let message: string = "Hello, World!";
console.log(message);

await fetch("https://ifconfig.co/json")
  .then((res) => res.json())
  .then((json) => console.log(json));

export {};
