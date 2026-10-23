import type { PracticeReport } from "../bindings/PracticeReport";
import type { PracticeScript } from "../bindings/PracticeScript";

export function mockPracticeScript(): PracticeScript {
  const items = [
    "你们的网络架构是怎么做高可用的？",
    "那你们的 SDN 控制器是怎么部署的？南向用的什么协议？",
    "控制器挂了还能转发吗？",
  ];
  return {
    kind: "questions",
    title: "questions-network.csv",
    items: items.map((text, i) => ({
      id: i + 1,
      text,
      speaker: null,
      startMs: null,
      endMs: null,
      isMe: false,
      isQuestion: true,
      reference: null,
      category: "SDN",
      followups: [],
    })),
    speakers: [],
    meSpeaker: null,
    needsClassification: false,
  };
}

export function mockPracticeReport(): PracticeReport {
  return {
    id: "demo",
    title: "questions-network.csv",
    startedAt: new Date().toISOString(),
    durationMs: 312_000,
    playback: "simulatedLatency",
    hintLevel: "headsOnly",
    simulated: true,
    questions: [
      {
        question: "你们的网络架构是怎么做高可用的？",
        answer: "我们主要是两地双活数据中心，嗯，流量分担。核心交换机做了堆叠……",
        reference: null,
        startLatencyMs: 1400,
        answerMs: 42_000,
        pointsDone: 4,
        pointsTotal: 4,
        fillers: [{ word: "嗯", count: 2 }],
        pace: 212,
        paceUnit: "cpm",
        feedback: ["漏掉了故障收敛时间这个要点。", "可以先给结论，再展开细节。", "结构清晰，关键词覆盖完整。"],
        followUp: "能具体说说当时遇到的最大困难吗？",
        skipped: false,
      },
    ],
    summary: { questions: 3, answered: 3, avgStartLatencyMs: 1800, avgCoverage: 0.83, totalFillers: 5, avgPace: 205 },
  };
}
