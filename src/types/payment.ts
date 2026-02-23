/**
 * 支付相关类型定义
 */

/** 支付方式 */
export type PaymentMethod = 'alipay' | 'wechat'

/** 支付状态 */
export type PaymentStatus = 'pending' | 'paid' | 'failed' | 'expired'

/** 订阅计划 */
export interface SubscriptionPlan {
  /** 计划 ID */
  id: string
  /** 计划名称 */
  name: string
  /** 价格（元） */
  price: number
  /** 原价（用于显示折扣） */
  originalPrice?: number
  /** 描述 */
  description: string
  /** 功能列表 */
  features: string[]
  /** 是否推荐 */
  isRecommended?: boolean
  /** 标签（如"最受欢迎"） */
  tag?: string
}

/** 支付订单 */
export interface PaymentOrder {
  /** 订单号 */
  order_no: string
  /** 金额（元） */
  amount: number
  /** 支付方式 */
  payment_method: PaymentMethod
  /** 订单状态 */
  status: PaymentStatus
  /** 支付链接 */
  pay_url?: string
  /** 二维码图片 URL */
  qr_code_url?: string
  /** 交易号 */
  trade_no?: string
  /** 创建时间 */
  created_at: string
  /** 支付时间 */
  paid_at?: string
}

/** 创建订单请求 */
export interface CreateOrderRequest {
  /** 计划 ID */
  plan_id: string
  /** 金额 */
  amount: number
  /** 支付方式 */
  payment_method: PaymentMethod
}

/** 创建订单响应 */
export interface CreateOrderResponse {
  /** 是否成功 */
  success: boolean
  /** 订单信息 */
  order?: PaymentOrder
  /** 错误信息 */
  error?: string
}

/** 查询订单响应 */
export interface QueryOrderResponse {
  /** 是否成功 */
  success: boolean
  /** 订单状态 */
  status?: PaymentStatus
  /** 是否已支付 */
  is_paid?: boolean
  /** 错误信息 */
  error?: string
}
