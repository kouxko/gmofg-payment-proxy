/*
 * ===========================================================================================
 * = COPYRIGHT
 *          PAX Computer Technology(Shenzhen) CO., LTD PROPRIETARY INFORMATION
 *   This software is supplied under the terms of a license agreement or nondisclosure
 *   agreement with PAX Computer Technology(Shenzhen) CO., LTD and may not be copied or
 *   disclosed except in accordance with the terms in that agreement.
 *     Copyright (C) 2022-? PAX Computer Technology(Shenzhen) CO., LTD All rights reserved.
 * Revision History:
 * Date	                Author	                Action
 * 20231010 	        JerryJia               	Create
 * ===========================================================================================
 */
package jp.gmofg.cps.https.msg;

public class BaseResult {
    private String orderDetailId;
    private String orderId;
    private String transTime;
    private String transStatus;

    public void setOrderDetailId(String orderDetailId) {
        this.orderDetailId = orderDetailId;
    }
    public String getOrderDetailId() {
        return orderDetailId;
    }

    public void setOrderId(String orderId) {
        this.orderId = orderId;
    }
    public String getOrderId() {
        return orderId;
    }

    public void setTransTime(String transTime) {
        this.transTime = transTime;
    }
    public String getTransTime() {
        return transTime;
    }


    public void setTransStatus(String transStatus) {
        this.transStatus = transStatus;
    }
    public String getTransStatus() {
        return transStatus;
    }
}
